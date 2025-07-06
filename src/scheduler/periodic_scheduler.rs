//! Periodic task scheduler for managing long-running background tasks

use anyhow::Result;
use log::{debug, error, warn};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Metrics tracking for periodic tasks
#[derive(Debug, Clone)]
pub struct TaskMetrics {
    pub recent_durations: VecDeque<Duration>,
    pub max_recent_durations: usize,
    pub slow_threshold: Duration,
}

impl TaskMetrics {
    pub fn new(max_recent: usize, slow_threshold: Duration) -> Self {
        Self {
            recent_durations: VecDeque::with_capacity(max_recent),
            max_recent_durations: max_recent,
            slow_threshold,
        }
    }

    pub fn add_duration(&mut self, duration: Duration) {
        if self.recent_durations.len() >= self.max_recent_durations {
            self.recent_durations.pop_front();
        }
        self.recent_durations.push_back(duration);
    }

    pub fn is_consistently_slow(&self) -> bool {
        if self.recent_durations.len() < 3 {
            return false;
        }

        self.recent_durations
            .iter()
            .all(|&d| d > self.slow_threshold)
    }

    pub fn average_duration(&self) -> Duration {
        if self.recent_durations.is_empty() {
            return Duration::ZERO;
        }

        let total: u64 = self
            .recent_durations
            .iter()
            .map(|d| d.as_millis() as u64)
            .sum();

        Duration::from_millis(total / self.recent_durations.len() as u64)
    }
}

/// State tracking for a periodic task
#[derive(Debug, Clone)]
pub struct TaskState {
    pub is_running: bool,
    pub last_start: Option<Instant>,
    pub last_completion: Option<Instant>,
    pub total_runs: u64,
    pub total_duration: Duration,
    pub metrics: TaskMetrics,
}

impl TaskState {
    pub fn new(metrics: TaskMetrics) -> Self {
        Self {
            is_running: false,
            last_start: None,
            last_completion: None,
            total_runs: 0,
            total_duration: Duration::ZERO,
            metrics,
        }
    }
}

/// Definition of a periodic task
pub struct PeriodicTask {
    pub name: String,
    pub interval: Duration,
    pub metrics: TaskMetrics,
    pub task: Box<
        dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
            + Send
            + Sync,
    >,
}

/// Scheduler for managing periodic tasks
pub struct PeriodicScheduler {
    tasks: Vec<PeriodicTask>,
    task_states: Arc<Mutex<HashMap<String, TaskState>>>,
    handles: Vec<JoinHandle<()>>,
}

impl PeriodicScheduler {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            task_states: Arc::new(Mutex::new(HashMap::new())),
            handles: Vec::new(),
        }
    }

    pub fn add_task(&mut self, task: PeriodicTask) {
        let name = task.name.clone();
        let metrics = task.metrics.clone();
        let task_states = self.task_states.clone();

        // Initialize task state
        tokio::spawn(async move {
            let mut states = task_states.lock().await;
            states.insert(name, TaskState::new(metrics));
        });

        self.tasks.push(task);
    }

    pub async fn start(&mut self) -> Result<()> {
        let tasks = std::mem::take(&mut self.tasks);
        for task in tasks {
            let handle = self.spawn_task(task).await?;
            self.handles.push(handle);
        }
        Ok(())
    }

    pub async fn stop(&mut self) {
        for handle in self.handles.drain(..) {
            handle.abort();
        }
    }

    async fn spawn_task(&self, task: PeriodicTask) -> Result<JoinHandle<()>> {
        let name = task.name.clone();
        let interval_duration = task.interval;
        let task_fn = task.task;
        let task_states = self.task_states.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);

            loop {
                interval.tick().await;

                // Check if already running
                {
                    let mut states = task_states.lock().await;
                    if let Some(state) = states.get_mut(&name) {
                        if state.is_running {
                            warn!("Task '{}' is still running, skipping", name);
                            continue;
                        }
                        state.is_running = true;
                        state.last_start = Some(Instant::now());
                    }
                }

                // Execute task
                let start = Instant::now();
                let result = task_fn().await;
                let duration = start.elapsed();

                // Update state and metrics
                {
                    let mut states = task_states.lock().await;
                    if let Some(state) = states.get_mut(&name) {
                        state.is_running = false;
                        state.last_completion = Some(Instant::now());
                        state.total_runs += 1;
                        state.total_duration += duration;
                        state.metrics.add_duration(duration);

                        // Log performance
                        if duration > interval_duration {
                            warn!(
                                "Task '{}' took {:?} (longer than interval {:?})",
                                name, duration, interval_duration
                            );
                        } else {
                            debug!("Task '{}' completed in {:?}", name, duration);
                        }
                    }
                }

                if let Err(e) = result {
                    error!("Task '{}' failed: {}", name, e);
                }
            }
        });

        Ok(handle)
    }

    pub async fn get_task_metrics(&self, task_name: &str) -> Option<TaskState> {
        let states = self.task_states.lock().await;
        states.get(task_name).cloned()
    }
}

impl Default for PeriodicScheduler {
    fn default() -> Self {
        Self::new()
    }
}
