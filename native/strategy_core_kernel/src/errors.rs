use std::error::Error;
use std::fmt::{Display, Formatter};

pub type KernelResult<T> = Result<T, KernelError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelError {
    message: String,
}

impl KernelError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for KernelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for KernelError {}
