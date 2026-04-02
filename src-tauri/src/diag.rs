use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata)
        .join("VoiceInput")
        .join("runtime.log")
}

pub fn write(event: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}][pid={}] {}", ts, pid, event);
    }
}

pub fn flatten_text(text: &str) -> String {
    text.replace('\r', "\\r").replace('\n', "\\n")
}

pub fn write_text(event: &str, text: &str) {
    write(&format!("{}:{}", event, flatten_text(text)));
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        write(&format!("panic: {}", info));
    }));
}

#[cfg(test)]
mod tests {
    use super::flatten_text;

    #[test]
    fn flatten_text_escapes_newlines() {
        assert_eq!(flatten_text("hello\r\nworld"), "hello\\r\\nworld");
    }
}
