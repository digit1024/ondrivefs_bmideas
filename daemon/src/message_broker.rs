use log::{debug, error, info};
use serde::{Deserialize, Serialize};
#[allow(dead_code)]
use std::sync::Arc;
use tokio::sync::broadcast;

/// Message types for internal communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppMessage {
    /// Sync status updates
    SyncStatusChanged {
        status: String,
        progress: Option<(u32, u32)>,
    },

    /// File operation events
    FileDownloaded {
        onedrive_id: String,
        local_path: String,
    },

    FileUploaded {
        onedrive_id: String,
        local_path: String,
    },

    FileDeleted {
        onedrive_id: String,
        path: String,
    },

    /// Authentication events
    AuthenticationChanged {
        is_authenticated: bool,
    },

    /// Connectivity events
    ConnectivityChanged {
        is_online: bool,
    },

    /// Conflict events
    ConflictDetected {
        onedrive_id: String,
        path: String,
        conflict_type: String,
    },

    /// Error events
    ErrorOccurred {
        component: String,
        error: String,
    },

    /// Queue status updates
    QueueStatusChanged {
        download_queue_size: u32,
        upload_queue_size: u32,
    },
}

/// Message broker for internal communication
pub struct MessageBroker {
    sender: broadcast::Sender<AppMessage>,
}
#[allow(dead_code)]
impl MessageBroker {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Send a message to all subscribers
    pub fn send(&self, message: AppMessage) -> Result<(), broadcast::error::SendError<AppMessage>> {
        debug!("📨 Sending message: {:?}", message);
        self.sender.send(message).map(|_| ())
    }

    /// Subscribe to messages
    pub fn subscribe(&self) -> broadcast::Receiver<AppMessage> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Clone for MessageBroker {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

/// Message handler trait for components that want to receive messages
#[allow(dead_code)]
pub trait MessageHandler {
    fn handle_message(&mut self, message: &AppMessage) -> Result<(), Box<dyn std::error::Error>>;
}

/// Message processor for handling messages in background
#[allow(dead_code)]
pub struct MessageProcessor {
    broker: Arc<MessageBroker>,
    handlers: Vec<Box<dyn MessageHandler + Send>>,
}
#[allow(dead_code)]
impl MessageProcessor {
    pub fn new(broker: Arc<MessageBroker>) -> Self {
        Self {
            broker,
            handlers: Vec::new(),
        }
    }

    /// Add a message handler
    pub fn add_handler(&mut self, handler: Box<dyn MessageHandler + Send>) {
        self.handlers.push(handler);
    }

    /// Start processing messages
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut receiver = self.broker.subscribe();

        info!(
            "🚀 Starting message processor with {} handlers",
            self.handlers.len()
        );

        while let Ok(message) = receiver.recv().await {
            debug!("📨 Processing message: {:?}", message);

            for handler in &mut self.handlers {
                if let Err(e) = handler.handle_message(&message) {
                    error!("❌ Handler error: {}", e);
                }
            }
        }

        Ok(())
    }
}
