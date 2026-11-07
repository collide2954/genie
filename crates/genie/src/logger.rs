use std::fmt;
use std::io::Write;
use std::time::Instant;

use log::{Level, LevelFilter, Log, Metadata, Record};

pub struct Logger {
    started: Instant,
    genie: LevelFilter,
    others: LevelFilter,
}

impl Logger {
    pub fn new(genie: LevelFilter) -> Self {
        Self {
            started: Instant::now(),
            genie,
            others: genie.min(LevelFilter::Warn),
        }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let limit = if metadata.target().starts_with("genie") {
            self.genie
        } else {
            self.others
        };
        metadata.level() <= limit
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let elapsed = self.started.elapsed().as_secs_f64();
            let text = line(elapsed, record.level(), record.target(), record.args());
            let _ = writeln!(std::io::stderr().lock(), "{text}");
        }
    }

    fn flush(&self) {}
}

pub fn level(value: Option<&str>) -> LevelFilter {
    value
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(LevelFilter::Warn)
}

pub fn line(elapsed: f64, level: Level, target: &str, message: &fmt::Arguments) -> String {
    format!("{elapsed:9.3} {level:5} {target}: {message}")
}

pub fn install() {
    let genie = level(std::env::var("GENIE_LOG").ok().as_deref());
    let logger: &'static Logger = Box::leak(Box::new(Logger::new(genie)));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(genie.max(LevelFilter::Warn));
    }
}

#[cfg(test)]
mod tests {
    use log::{Level, LevelFilter, Log, Metadata};

    use super::{Logger, level, line};

    fn enabled(logger: &Logger, target: &str, level: Level) -> bool {
        logger.enabled(&Metadata::builder().target(target).level(level).build())
    }

    #[test]
    fn levels_come_from_genie_log() {
        assert_eq!(level(None), LevelFilter::Warn);
        assert_eq!(level(Some("debug")), LevelFilter::Debug);
        assert_eq!(level(Some(" INFO ")), LevelFilter::Info);
        assert_eq!(level(Some("loud")), LevelFilter::Warn);
    }

    #[test]
    fn other_crates_stay_at_warnings() {
        let logger = Logger::new(LevelFilter::Debug);
        assert!(enabled(&logger, "genie::app", Level::Debug));
        assert!(enabled(&logger, "genie_formats::fai", Level::Debug));
        assert!(!enabled(&logger, "wgpu_core::device", Level::Info));
        assert!(enabled(&logger, "wgpu_core::device", Level::Warn));
        let quiet = Logger::new(LevelFilter::Error);
        assert!(!enabled(&quiet, "wgpu_core::device", Level::Warn));
    }

    #[test]
    fn lines_show_time_level_and_target() {
        assert_eq!(
            line(
                1.5,
                Level::Info,
                "genie::app",
                &format_args!("Opened {}", "ref.fa")
            ),
            "    1.500 INFO  genie::app: Opened ref.fa"
        );
    }
}
