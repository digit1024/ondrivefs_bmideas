use anyhow::{Context, Result};
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::append::rolling_file::policy::compound::{
    CompoundPolicy, roll::fixed_window::FixedWindowRoller, trigger::size::SizeTrigger,
};
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use std::fs;
use std::path::PathBuf;

/// Default log file size in bytes (5MB)
const DEFAULT_LOG_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Number of log files to keep
const LOG_FILE_COUNT: u32 = 3;

/// Log directory name
const LOG_DIR: &str = "logs";

/// Log file name pattern
const LOG_FILE_PATTERN: &str = "daemon.{}.log.gz";

/// Console log pattern
const CONSOLE_LOG_PATTERN: &str = "{h({l})} {d(%Y-%m-%d %H:%M:%S)} {M} - {m}{n}";

/// File log pattern
const FILE_LOG_PATTERN: &str = "{d} {l}::{m}{n}";

/// Setup logging configuration for the application
pub async fn setup_logging(log_dir: &PathBuf) -> Result<()> {
    // Ensure logs directory exists
    let logs_path = log_dir.join(LOG_DIR);
    fs::create_dir_all(&logs_path)
        .with_context(|| format!("Failed to create logs directory: {}", logs_path.display()))?;

    // Create console appender
    let stdout = create_console_appender()?;

    // Create rolling file appender
    let file = create_rolling_file_appender(&logs_path)?;

    // Build configuration
    let config = build_log_config(stdout, file)?;

    // Initialize logging
    log4rs::init_config(config).context("Failed to initialize logging configuration")?;

    Ok(())
}

/// Create console appender with custom pattern
fn create_console_appender() -> Result<ConsoleAppender> {
    Ok(ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(CONSOLE_LOG_PATTERN)))
        .build())
}

/// Create rolling file appender with size-based rotation
fn create_rolling_file_appender(logs_path: &PathBuf) -> Result<RollingFileAppender> {
    // Fixed window roller: keep compressed log files
    let roller = FixedWindowRoller::builder()
        .base(1)
        .build(LOG_FILE_PATTERN, LOG_FILE_COUNT)
        .map_err(|e| anyhow::anyhow!("Failed to create log roller: {}", e))?;

    // Size trigger: roll when file reaches size limit
    let trigger = SizeTrigger::new(DEFAULT_LOG_FILE_SIZE);

    // Compound policy: size-based rolling with fixed window
    let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));

    // Rolling file appender
    let log_file_path = logs_path.join("daemon.log");
    RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(FILE_LOG_PATTERN)))
        .build(log_file_path.to_str().unwrap(), Box::new(policy))
        .map_err(|e| anyhow::anyhow!("Failed to create rolling file appender: {}", e))
}

/// Build logging configuration
fn build_log_config(stdout: ConsoleAppender, file: RollingFileAppender) -> Result<Config> {
    Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("file", Box::new(file)))
        .logger(
            log4rs::config::Logger::builder()
                .appender("stdout")
                .appender("file")
                .additive(false)
                .build("sqlx_core::logger", LevelFilter::Off)
        )
        .build(
            Root::builder()
                .appender("stdout")
                .appender("file")
                .build(LevelFilter::Debug),
        )
        .map_err(|e| anyhow::anyhow!("Failed to build logging configuration: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_setup_logging() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        let result = setup_logging(&log_dir).await;
        assert!(result.is_ok());

        // Verify logs directory was created
        let logs_path = log_dir.join(LOG_DIR);
        assert!(logs_path.exists());
        assert!(logs_path.is_dir());
    }

    #[test]
    fn test_console_appender_creation() {
        let result = create_console_appender();
        assert!(result.is_ok());
    }

    #[test]
    fn test_rolling_file_appender_creation() {
        let temp_dir = TempDir::new().unwrap();
        let logs_path = temp_dir.path().to_path_buf();

        let result = create_rolling_file_appender(&logs_path);
        assert!(result.is_ok());
    }
}
