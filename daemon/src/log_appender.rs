use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::append::rolling_file::policy::compound::{
    CompoundPolicy,
    roll::fixed_window::FixedWindowRoller,
    trigger::size::SizeTrigger,
};
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use std::path::PathBuf;
use anyhow::Result;
use std::fs;

pub async fn setup_logging(log_dir: &PathBuf) -> Result<()> {
    // Ensure logs directory exists
    fs::create_dir_all(log_dir.join("logs"))?;

    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{h({l})} {d(%Y-%m-%d %H:%M:%S)} {M} - {m}{n}")))
        .build();

    // Fixed window roller: keep 3 compressed log files, 5MB each
    let roller = FixedWindowRoller::builder()
        .base(1)
        .build("logs/daemon.{}.log.gz", 3)?; // pattern and count in build()

    // Size trigger: roll when file reaches 5MB
    let trigger = SizeTrigger::new(50 * 1024 * 1024); // 5MB

    // Compound policy: size-based rolling with fixed window
    let policy = CompoundPolicy::new(
        Box::new(trigger),
        Box::new(roller),
    );

    // Rolling file appender
    let file = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} {l}::{m}{n}")))
        .build(log_dir.join("logs").join("daemon.log").to_str().unwrap(), Box::new(policy))?;

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("file", Box::new(file)))
        .build(
            Root::builder()
                .appender("stdout")
                .appender("file")
                .build(LevelFilter::Debug),
        )?;

    log4rs::init_config(config)?;
    Ok(())
}