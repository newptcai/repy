use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Warn as u8);

pub fn init(level: LogLevel) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn error(message: impl AsRef<str>) {
    log(LogLevel::Error, "error", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    log(LogLevel::Warn, "warn", message.as_ref());
}

pub fn info(message: impl AsRef<str>) {
    log(LogLevel::Info, "info", message.as_ref());
}

pub fn debug(message: impl AsRef<str>) {
    log(LogLevel::Debug, "debug", message.as_ref());
}

fn log(level: LogLevel, label: &str, message: &str) {
    let current = LOG_LEVEL.load(Ordering::Relaxed);
    if current >= level as u8 {
        eprintln!("[{}] {}", label, message);
    }
}
