//! A backend that never leaves the process, for tests of this crate and of its callers.

use std::sync::Mutex;

use crate::backend::{Backend, BackendError};

/// Answers every prompt with one canned reply, or fails with one canned message, and remembers
/// the prompts it was given.
pub struct FakeBackend {
    reply: Result<String, String>,
    prompts: Mutex<Vec<String>>,
}

impl FakeBackend {
    pub fn replying(reply: &str) -> Self {
        FakeBackend {
            reply: Ok(reply.to_string()),
            prompts: Mutex::default(),
        }
    }

    pub fn failing(message: &str) -> Self {
        FakeBackend {
            reply: Err(message.to_string()),
            prompts: Mutex::default(),
        }
    }

    pub fn prompts(&self) -> Vec<String> {
        self.prompts
            .lock()
            .expect("prompt log is never poisoned")
            .clone()
    }
}

impl Backend for FakeBackend {
    fn complete(&self, prompt: &str) -> Result<String, BackendError> {
        self.prompts
            .lock()
            .expect("prompt log is never poisoned")
            .push(prompt.to_string());
        self.reply.clone().map_err(|stderr| BackendError::Failed {
            program: "fake".to_string(),
            status: Some(1),
            stderr,
        })
    }
}
