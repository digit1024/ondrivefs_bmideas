#![allow(dead_code)]
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use onedrive_sync_daemon::app_state::AppState;
use onedrive_sync_daemon::log_appender::setup_logging;
use onedrive_sync_daemon::onedrive_service::onedrive_client::mock::MockOneDriveClient;
use onedrive_sync_daemon::onedrive_service::onedrive_client::OneDriveClientTrait;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Global test environment that persists across all tests
pub static TEST_ENV: Lazy<Arc<Mutex<TestEnv>>> = Lazy::new(|| {
    Arc::new(Mutex::new(
        TestEnv::new().expect("Failed to create test environment"),
    ))
});

/// Test environment that manages persistent directories and database
pub struct TestEnv {
    #[allow(dead_code)]
    /// Root temporary directory (persisted)
    temp_dir: TempDir,
    #[allow(dead_code)]
    /// Path to the test database
    db_path: PathBuf,
    /// Test data directory
    data_dir: PathBuf,
    /// Test config directory
    config_dir: PathBuf,
    /// Test cache directory
    cache_dir: PathBuf,
    /// Shared AppState instance
    app_state: Option<Arc<AppState>>,
}

impl TestEnv {
    /// Create a new test environment with persistent directories
    fn new() -> Result<Self> {
        // Create persistent temp directory
        let temp_dir = TempDir::new().context("Failed to create temp directory")?;

        // Set up directory structure
        let data_dir = temp_dir.path().join("data");
        let config_dir = temp_dir.path().join("config");
        let cache_dir = temp_dir.path().join("cache");

        // Create directories
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&config_dir)?;
        std::fs::create_dir_all(&cache_dir)?;

        // Create onedrive-sync subdirectories
        let onedrive_data_dir = data_dir.join("onedrive-sync");
        std::fs::create_dir_all(&onedrive_data_dir)?;
        std::fs::create_dir_all(onedrive_data_dir.join("downloads"))?;
        std::fs::create_dir_all(onedrive_data_dir.join("uploads"))?;
        std::fs::create_dir_all(onedrive_data_dir.join("local"))?;

        let onedrive_config_dir = config_dir.join("onedrive-sync");
        std::fs::create_dir_all(&onedrive_config_dir)?;

        let onedrive_cache_dir = cache_dir.join("onedrive-sync");
        std::fs::create_dir_all(&onedrive_cache_dir)?;

        let db_path = onedrive_data_dir.join("onedrive.db");

        println!(
            "🧪 Test environment created at: {}",
            temp_dir.path().display()
        );
        println!("📁 Data directory: {}", data_dir.display());
        println!("⚙️  Config directory: {}", config_dir.display());
        println!("💾 Cache directory: {}", cache_dir.display());
        println!("🗄️  Database path: {}", db_path.display());

        Ok(Self {
            temp_dir,
            db_path,
            data_dir,
            config_dir,
            cache_dir,
            app_state: None,
        })
    }

    /// Get or create the shared AppState instance with real OneDrive client
    pub async fn get_app_state(&mut self) -> Result<Arc<AppState>> {
        if let Some(ref app_state) = self.app_state {
            return Ok(app_state.clone());
        }

        // Set environment variables for ProjectConfig
        std::env::set_var("XDG_DATA_HOME", &self.data_dir);
        std::env::set_var("XDG_CONFIG_HOME", &self.config_dir);
        std::env::set_var("XDG_CACHE_HOME", &self.cache_dir);

        println!("🚀 Initializing AppState for tests...");

        // Create AppState
        let app_state = AppState::new().await.context("Failed to create AppState")?;

        // Setup logging for tests
        setup_logging(&self.data_dir)
            .await
            .context("Failed to setup logging")?;

        // Initialize database schema
        app_state
            .persistency()
            .init_database()
            .await
            .context("Failed to initialize database")?;

        let app_state = Arc::new(app_state);
        self.app_state = Some(app_state.clone());

        println!("✅ AppState initialized successfully");

        Ok(app_state)
    }

    /// Get or create an AppState instance with mock OneDrive client
    pub async fn get_app_state_with_mock(&mut self) -> Result<Arc<AppState>> {
        // Set environment variables for ProjectConfig
        std::env::set_var("XDG_DATA_HOME", &self.data_dir);
        std::env::set_var("XDG_CONFIG_HOME", &self.config_dir);
        std::env::set_var("XDG_CACHE_HOME", &self.cache_dir);

        println!("🚀 Initializing AppState with mock OneDrive client for tests...");

        // Create mock OneDrive client
        let mock_client = Arc::new(MockOneDriveClient::new()) as Arc<dyn OneDriveClientTrait>;

        // Create AppState with mock client
        let app_state = AppState::with_onedrive_client(mock_client)
            .await
            .context("Failed to create AppState with mock")?;

        // Setup logging for tests
        setup_logging(&self.data_dir)
            .await
            .context("Failed to setup logging")?;

        // Initialize database schema
        app_state
            .persistency()
            .init_database()
            .await
            .context("Failed to initialize database")?;

        println!("✅ AppState with mock OneDrive client initialized successfully");

        Ok(Arc::new(app_state))
    }

    /// Get the test database path
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Get the test data directory
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get the test config directory
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    /// Get the test cache directory
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Get the root temp directory path for inspection
    pub fn temp_dir_path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Clear specific tables (useful between test groups)
    pub async fn clear_processing_items(&self) -> Result<()> {
        if let Some(ref app_state) = self.app_state {
            app_state
                .persistency()
                .processing_item_repository()
                .clear_all_items()
                .await?;
            println!("🧹 Cleared processing_items table");
        }
        Ok(())
    }

    /// Clear all data from specific repositories (useful for test isolation)
    pub async fn clear_all_data(&self) -> Result<()> {
        if let Some(ref app_state) = self.app_state {
            // Clear processing items
            app_state
                .persistency()
                .processing_item_repository()
                .clear_all_items()
                .await?;

            // You can add more repository clears here as needed

            println!("🧹 Cleared all test data");
        }
        Ok(())
    }
}

/// Helper macro to get AppState in tests
#[macro_export]
macro_rules! get_test_app_state {
    () => {{
        use $crate::common::setup::TEST_ENV;
        let mut env = TEST_ENV.lock().await;
        env.get_app_state().await
    }};
}

/// Helper macro to get AppState with mock OneDrive client in tests
#[macro_export]
macro_rules! get_test_app_state_with_mock {
    () => {{
        use $crate::common::setup::TEST_ENV;
        let mut env = TEST_ENV.lock().await;
        env.get_app_state_with_mock().await
    }};
}

/// Helper macro to setup test with AppState
#[macro_export]
macro_rules! setup_test {
    () => {{
        use $crate::common::setup::TEST_ENV;
        let env = TEST_ENV.lock().await;
        println!(
            "📍 Test using environment at: {}",
            env.temp_dir_path().display()
        );
        drop(env);
        get_test_app_state!()
    }};
}

/// Helper macro to setup test with mock AppState
#[macro_export]
macro_rules! setup_test_with_mock {
    () => {{
        use $crate::common::setup::TEST_ENV;
        let env = TEST_ENV.lock().await;
        println!(
            "📍 Test using mock environment at: {}",
            env.temp_dir_path().display()
        );
        drop(env);
        get_test_app_state_with_mock!()
    }};
}
