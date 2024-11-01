use alloy::hex::FromHexError;
use anyhow::Result as AnyhowResult;
use std::error::Error;
use std::fmt::{Debug, Display};

pub type Result<T> = AnyhowResult<T, HPError>;

pub struct HPError {
    message: String,
    is_honeypot: Option<bool>,
}

impl HPError {
    pub fn new(message: String, is_honeypot: Option<bool>) -> Self {
        Self {
            message,
            is_honeypot,
        }
    }

    pub fn err_msg(message: String) -> Self {
        Self::new(message, None)
    }

    pub fn parse_error(e: FromHexError) -> Self {
        Self::new(e.to_string(), None)
    }

    pub fn rpc_error(e: impl Display) -> Self {
        Self::new(e.to_string(), None)
    }

    pub fn error(e: impl Error) -> Self {
        Self::new(e.to_string(), None)
    }
}

impl Display for HPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Debug for HPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HPError {{ message: {}, is_honeypot: {:?} }}",
            self.message, self.is_honeypot
        )
    }
}

impl Error for HPError {}
