//! A mutex that tells the server log when someone keeps it too long, and where they took it.
//! The library (`App::db`) uses it: every request waits for that lock, so one slow holder makes
//! the whole web app hang, and the log line names the code to fix.

use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Held longer than this → one line in the server log.
const LIMIT: Duration = Duration::from_secs(1);

pub struct HeldMutex<T> {
    inner: Mutex<T>,
    limit: Duration,
}

pub struct HeldGuard<'a, T> {
    guard: Option<MutexGuard<'a, T>>,
    since: Instant,
    at: &'static Location<'static>,
    limit: Duration,
}

impl<T> HeldMutex<T> {
    pub fn new(v: T) -> Self { Self::with_limit(v, LIMIT) }

    pub fn with_limit(v: T, limit: Duration) -> Self { HeldMutex { inner: Mutex::new(v), limit } }

    /// Like `Mutex::lock`; the caller's file and line are kept for the log.
    #[track_caller]
    pub fn lock(&self) -> Result<HeldGuard<'_, T>, PoisonError<MutexGuard<'_, T>>> {
        let at = Location::caller();
        let guard = self.inner.lock()?;
        Ok(HeldGuard { guard: Some(guard), since: Instant::now(), at, limit: self.limit })
    }
}

impl<T> Deref for HeldGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.guard.as_ref().expect("held") }
}

impl<T> DerefMut for HeldGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.guard.as_mut().expect("held") }
}

impl<T> Drop for HeldGuard<'_, T> {
    fn drop(&mut self) {
        // Let go first, so the others are not kept waiting for the log line.
        drop(self.guard.take());
        let held = self.since.elapsed();
        if held >= self.limit {
            elog!("the library was locked for {:.1} s by {}:{}", held.as_secs_f64(), self.at.file(), self.at.line());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_a_long_hold_with_its_place() {
        let m = HeldMutex::with_limit(1, Duration::ZERO);
        { let mut g = m.lock().unwrap(); *g += 1; }
        assert_eq!(*m.lock().unwrap(), 2);
        let lines = crate::admin::log::lines();
        assert!(lines.iter().any(|(_, l)| l.starts_with("the library was locked for") && l.contains("held.rs:")), "{lines:?}");
    }
}
