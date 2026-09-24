//! CLI logging through the existing `log` facade. Message patterns use Perex;
//! the runtime does not depend on this logger or its environment parsing.
use log::{LevelFilter, Log, Metadata, Record};
use perry_perex::tooling::Regex;
use std::{io::Write, sync::OnceLock};

struct Logger {
    directives: Vec<(String, LevelFilter)>,
    pattern: Option<Regex>,
}
impl Logger {
    fn parse(value: &str) -> Self {
        let (directives, pattern) = value
            .split_once('/')
            .map_or((value, None), |(d, p)| (d, Some(p)));
        let mut levels = Vec::new();
        for directive in directives
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let (target, level) = if let Some((target, level)) = directive.split_once('=') {
                let Ok(level) = level.trim().parse() else {
                    eprintln!("Ignoring invalid RUST_LOG directive: {directive}");
                    continue;
                };
                (target.trim(), level)
            } else if let Ok(level) = directive.parse() {
                ("", level)
            } else {
                (directive, LevelFilter::Trace)
            };
            levels.retain(|(old, _)| old != target);
            levels.push((target.to_string(), level));
        }
        if levels.is_empty() {
            levels.push((String::new(), LevelFilter::Error));
        }
        levels.sort_by_key(|(target, _)| std::cmp::Reverse(target.len()));
        let pattern = pattern.and_then(|p| match Regex::new(p) {
            Ok(regex) => Some(regex),
            Err(error) => {
                eprintln!("Ignoring invalid RUST_LOG pattern: {error}");
                None
            }
        });
        Self {
            directives: levels,
            pattern,
        }
    }
}
impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.directives
            .iter()
            .find(|(prefix, _)| metadata.target().starts_with(prefix))
            .is_some_and(|(_, level)| metadata.level() <= *level)
    }
    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if self.pattern.as_ref().is_some_and(|p| !p.is_match(&message)) {
            return;
        }
        let _ = writeln!(
            std::io::stderr().lock(),
            "[{} {:5} {}] {}",
            crate::utc::now_rfc3339(),
            record.level(),
            record.target(),
            message
        );
    }
    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}
pub fn init() {
    static LOGGER: OnceLock<Logger> = OnceLock::new();
    let logger =
        LOGGER.get_or_init(|| Logger::parse(&std::env::var("RUST_LOG").unwrap_or_default()));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(
            logger
                .directives
                .iter()
                .map(|(_, l)| *l)
                .max()
                .unwrap_or(LevelFilter::Error),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn enabled(logger: &Logger, target: &str, level: log::Level) -> bool {
        logger.enabled(&Metadata::builder().target(target).level(level).build())
    }
    #[test]
    fn prefix_filters_and_default() {
        let logger = Logger::parse("warn,perry=debug,perry::noisy=off");
        assert!(enabled(&logger, "perry::compile", log::Level::Debug));
        assert!(!enabled(&logger, "perry::noisy", log::Level::Error));
        assert!(!enabled(&logger, "other", log::Level::Info));
        assert!(enabled(&Logger::parse(""), "other", log::Level::Error));
        assert!(!enabled(&Logger::parse(""), "other", log::Level::Warn));
    }
    #[test]
    fn message_filter_uses_perex() {
        let logger = Logger::parse("debug/compile [0-9]+");
        let pattern = logger.pattern.unwrap();
        assert!(pattern.is_match("compile 42"));
        assert!(!pattern.is_match("compile nope"));
    }
}
