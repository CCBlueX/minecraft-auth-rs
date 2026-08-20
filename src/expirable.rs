use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::error::Result;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_millis() as i64
}

/// A value with a known expiry.
pub trait Expirable {
    fn expire_time_ms(&self) -> i64;

    fn is_expired(&self) -> bool {
        self.expire_time_ms() <= now_ms()
    }
}

/// A lazily refreshed, expiry-aware cache for one pipeline stage.
///
/// `get_up_to_date` holds the lock for the whole refresh, so concurrent
/// callers coalesce onto a single in-flight request instead of firing one
/// each.
pub struct Holder<T> {
    state: Mutex<Option<T>>,
}

impl<T: Expirable + Clone> Holder<T> {
    pub fn empty() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    pub fn with_value(value: T) -> Self {
        Self {
            state: Mutex::new(Some(value)),
        }
    }

    pub async fn cached(&self) -> Option<T> {
        self.state.lock().await.clone()
    }

    pub async fn get_up_to_date<F, Fut>(&self, refresh: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut guard = self.state.lock().await;
        let is_stale = match guard.as_ref() {
            Some(value) => value.is_expired(),
            None => true,
        };
        if is_stale {
            let fresh = refresh().await?;
            *guard = Some(fresh.clone());
            Ok(fresh)
        } else {
            Ok(guard.clone().expect("checked above"))
        }
    }
}
