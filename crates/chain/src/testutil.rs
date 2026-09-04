//! Test-only transport doubles.

use crate::{ChainError, EvmClient, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;

/// A canned [`EvmClient`]: each `request` records the call and pops the next
/// scripted reply, in order.
pub struct MockRpc {
    pub calls: Mutex<Vec<(String, Value)>>,
    replies: Mutex<Vec<Result<Value>>>,
}

impl MockRpc {
    /// Build a mock that will answer calls with `replies`, front to back.
    #[must_use]
    pub fn new(replies: Vec<Result<Value>>) -> Self {
        Self {
            calls: Mutex::new(vec![]),
            replies: Mutex::new(replies.into_iter().rev().collect()),
        }
    }

    /// The methods requested so far, in order.
    #[must_use]
    pub fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }
}

#[async_trait]
impl EvmClient for MockRpc {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.calls.lock().unwrap().push((method.to_owned(), params));
        self.replies
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| Err(ChainError::Decode("mock: no more replies".into())))
    }
}
