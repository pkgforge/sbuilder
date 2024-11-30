use std::sync::mpsc::Sender;

pub enum LogMessage {
    Info(String),
    Error(String),
    Warn(String),
    Success(String),
    CustomError(String),
    Done,
}

#[derive(Clone)]
pub struct Logger {
    sender: Sender<LogMessage>,
}

impl Logger {
    pub fn new(sender: Sender<LogMessage>) -> Self {
        Self { sender }
    }

    pub fn info(&self, msg: &str) {
        let _ = self.sender.send(LogMessage::Info(msg.to_string()));
    }

    pub fn warn(&self, msg: &str) {
        let _ = self.sender.send(LogMessage::Warn(msg.to_string()));
    }

    pub fn error(&self, msg: &str) {
        let _ = self.sender.send(LogMessage::Error(msg.to_string()));
    }

    pub fn success(&self, msg: &str) {
        let _ = self.sender.send(LogMessage::Success(msg.to_string()));
    }

    pub fn custom_error(&self, msg: &str) {
        let _ = self.sender.send(LogMessage::CustomError(msg.to_string()));
    }

    pub fn done(&self) {
        let _ = self.sender.send(LogMessage::Done);
    }
}
