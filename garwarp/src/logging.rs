use std::time::{SystemTime, UNIX_EPOCH};

pub fn info(message: &str) {
    emit("info", message);
}

pub fn warn(message: &str) {
    emit("warn", message);
}

pub fn error(message: &str) {
    emit("error", message);
}

fn emit(level: &str, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    eprintln!("ts={timestamp} level={level} component=garwarp msg={message}");
}
