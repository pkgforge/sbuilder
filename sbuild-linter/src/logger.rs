use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc::Sender, Arc, Mutex},
};

pub enum LogMessage {
    Info(String),
    Error(String),
    Warn(String),
    Success(String),
    CustomError(String),
    Done,
}

#[derive(Clone)]
pub struct LogManager {
    sender: Sender<LogMessage>,
}

impl LogManager {
    pub fn new(sender: Sender<LogMessage>) -> Self {
        Self { sender }
    }

    pub fn done(&self) {
        let _ = self.sender.send(LogMessage::Done);
    }

    pub fn create_logger<P: AsRef<Path>>(&self, file_path: Option<P>) -> TaskLogger {
        let file = if let Some(file_path) = file_path {
            let file_path = file_path.as_ref();
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)
                .unwrap();
            Some(Arc::new(Mutex::new(LogFile {
                file,
                path: file_path.to_path_buf(),
            })))
        } else {
            None
        };
        TaskLogger {
            sender: self.sender.clone(),
            file,
        }
    }
}

#[derive(Clone)]
pub struct TaskLogger {
    sender: Sender<LogMessage>,
    file: Option<Arc<Mutex<LogFile>>>,
}

struct LogFile {
    file: File,
    path: PathBuf,
}

impl TaskLogger {
    fn write_to_file(&self, msg: &str) {
        if let Some(file) = &self.file {
            if let Ok(mut file_guard) = file.lock() {
                let _ = writeln!(file_guard.file, "{}", msg);
            }
        }
    }

    pub fn move_log_file<P: AsRef<Path>>(&self, new_path: P) -> std::io::Result<()> {
        if let Some(file) = &self.file {
            let mut file_guard = file.lock().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to acquire lock on log file",
                )
            })?;

            file_guard.file.flush()?;
            let old_path = file_guard.path.clone();

            drop(file_guard);

            if old_path.exists() {
                fs::copy(&old_path, &new_path)?;
                fs::remove_file(&old_path)?;
            }

            let new_file = OpenOptions::new().append(true).open(new_path.as_ref())?;

            if let Ok(mut file_guard) = file.lock() {
                file_guard.file = new_file;
                file_guard.path = new_path.as_ref().to_path_buf();
            }
        }
        Ok(())
    }

    pub fn info(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.write_to_file(&msg);
        let _ = self.sender.send(LogMessage::Info(msg.to_string()));
    }

    pub fn warn(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.write_to_file(&msg);
        let _ = self.sender.send(LogMessage::Warn(msg.to_string()));
    }

    pub fn error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.write_to_file(&msg);
        let _ = self.sender.send(LogMessage::Error(msg.to_string()));
    }

    pub fn success(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.write_to_file(&msg);
        let _ = self.sender.send(LogMessage::Success(msg.to_string()));
    }

    pub fn custom_error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.write_to_file(&msg);
        let _ = self.sender.send(LogMessage::CustomError(msg.to_string()));
    }
}
