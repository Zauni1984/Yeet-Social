//! Account session state machine.
//!
//! The session tracks whether the account is locked or unlocked.
//! The encryption key only exists in memory while unlocked and is
//! automatically zeroized on logout, timeout, or drop.

use std::time::Duration;
use web_time::Instant;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{AccountError, AccountResult};

/// Default session timeout: 5 minutes of inactivity.
///
/// Defense-in-depth floor, *not* the user-facing lock. The user-facing
/// lock is driven by `dontyeet-ui::idle::AutoLockTimeout` (3-minute
/// default, configurable in Settings) which fires on actual DOM input
/// events and locks first. This timer only kicks in if the UI clock
/// somehow doesn't (stalled `spawn_local`, hung page) — at which point
/// the next call into `Session::encryption_key()` discovers the gap
/// and auto-locks.
pub const DEFAULT_SESSION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Encryption key held in memory only while unlocked.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct SessionKey(Vec<u8>);

/// The three possible account states.
enum SessionState {
    /// No account has been created yet.
    NoAccount,
    /// Account exists but is locked (key not in memory).
    Locked,
    /// Account is unlocked — encryption key is live in memory.
    Unlocked {
        key: SessionKey,
        last_activity: Instant,
    },
}

/// Account session — tracks authentication state.
///
/// This is the gatekeeper: the encryption key needed to read the
/// mnemonic is only accessible through this struct, and only when
/// the session is in the `Unlocked` state and not timed out.
pub struct Session {
    state: SessionState,
    /// How long the session can stay idle before auto-locking.
    timeout: Duration,
}

impl Session {
    /// Create a session in the `NoAccount` state with default timeout.
    #[must_use]
    pub fn no_account() -> Self {
        Self {
            state: SessionState::NoAccount,
            timeout: DEFAULT_SESSION_TIMEOUT,
        }
    }

    /// Create a session in the `Locked` state with default timeout.
    #[must_use]
    pub fn locked() -> Self {
        Self {
            state: SessionState::Locked,
            timeout: DEFAULT_SESSION_TIMEOUT,
        }
    }

    /// Set the session timeout duration.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Get the current session timeout duration.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Whether no account exists yet.
    #[must_use]
    pub fn is_no_account(&self) -> bool {
        matches!(self.state, SessionState::NoAccount)
    }

    /// Whether the account is locked.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        matches!(self.state, SessionState::Locked)
    }

    /// Whether the account is unlocked.
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        matches!(self.state, SessionState::Unlocked { .. })
    }

    /// Transition: `NoAccount` → `Locked` (after account creation).
    ///
    /// # Errors
    /// Returns `AccountError::AlreadyExists` if not in `NoAccount` state.
    pub fn mark_created(&mut self) -> AccountResult<()> {
        if !self.is_no_account() {
            return Err(AccountError::AlreadyExists);
        }
        self.state = SessionState::Locked;
        Ok(())
    }

    /// Transition: `Locked` → `Unlocked` (after successful login).
    ///
    /// # Errors
    /// Returns `AccountError::NotFound` if no account exists.
    pub fn unlock(&mut self, encryption_key: Vec<u8>) -> AccountResult<()> {
        if self.is_no_account() {
            return Err(AccountError::NotFound);
        }
        self.state = SessionState::Unlocked {
            key: SessionKey(encryption_key),
            last_activity: Instant::now(),
        };
        Ok(())
    }

    /// Transition: `Unlocked` → `Locked` (logout).
    ///
    /// The encryption key is zeroized when the old state is dropped.
    ///
    /// # Errors
    /// Returns `AccountError::NotAuthenticated` if not unlocked.
    pub fn lock(&mut self) -> AccountResult<()> {
        if !self.is_unlocked() {
            return Err(AccountError::NotAuthenticated);
        }
        self.state = SessionState::Locked;
        Ok(())
    }

    /// Transition: any → `NoAccount` (account deletion).
    pub fn mark_deleted(&mut self) {
        self.state = SessionState::NoAccount;
    }

    /// Borrow the encryption key (only available when unlocked and not
    /// timed out).
    ///
    /// Also updates the last-activity timestamp so the timeout resets
    /// on every use.
    ///
    /// # Errors
    /// Returns `AccountError::NotAuthenticated` if locked or expired.
    pub fn encryption_key(&mut self) -> AccountResult<&[u8]> {
        // Check for timeout first — auto-lock if expired.
        if let SessionState::Unlocked { last_activity, .. } = &self.state
            && last_activity.elapsed() > self.timeout
        {
            self.state = SessionState::Locked;
            return Err(AccountError::NotAuthenticated);
        }

        match &mut self.state {
            SessionState::Unlocked { key, last_activity } => {
                *last_activity = Instant::now();
                Ok(&key.0)
            }
            _ => Err(AccountError::NotAuthenticated),
        }
    }

    /// Check whether the session has timed out without auto-locking.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        match &self.state {
            SessionState::Unlocked { last_activity, .. } => last_activity.elapsed() > self.timeout,
            _ => false,
        }
    }

    /// Touch the session to reset the inactivity timer.
    pub fn touch(&mut self) {
        if let SessionState::Unlocked { last_activity, .. } = &mut self.state {
            *last_activity = Instant::now();
        }
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.state {
            SessionState::NoAccount => write!(f, "Session(NoAccount)"),
            SessionState::Locked => write!(f, "Session(Locked)"),
            SessionState::Unlocked { .. } => write!(f, "Session(Unlocked)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_no_account() {
        let s = Session::no_account();
        assert!(s.is_no_account());
        assert!(!s.is_locked());
        assert!(!s.is_unlocked());
    }

    #[test]
    fn create_transitions_to_locked() {
        let mut s = Session::no_account();
        s.mark_created().expect("create");
        assert!(s.is_locked());
    }

    #[test]
    fn create_when_exists_fails() {
        let mut s = Session::locked();
        assert!(s.mark_created().is_err());
    }

    #[test]
    fn unlock_transitions_to_unlocked() {
        let mut s = Session::locked();
        s.unlock(vec![1, 2, 3]).expect("unlock");
        assert!(s.is_unlocked());
    }

    #[test]
    fn unlock_no_account_fails() {
        let mut s = Session::no_account();
        assert!(s.unlock(vec![1, 2, 3]).is_err());
    }

    #[test]
    fn lock_transitions_to_locked() {
        let mut s = Session::locked();
        s.unlock(vec![1, 2, 3]).expect("unlock");
        s.lock().expect("lock");
        assert!(s.is_locked());
    }

    #[test]
    fn encryption_key_only_when_unlocked() {
        let mut s = Session::locked();
        assert!(s.encryption_key().is_err());

        s.unlock(vec![0xAB, 0xCD]).expect("unlock");
        assert_eq!(s.encryption_key().expect("key"), &[0xAB, 0xCD]);

        s.lock().expect("lock");
        assert!(s.encryption_key().is_err());
    }

    #[test]
    fn delete_resets_to_no_account() {
        let mut s = Session::locked();
        s.unlock(vec![1, 2, 3]).expect("unlock");
        s.mark_deleted();
        assert!(s.is_no_account());
    }
}

// Rust guideline compliant 2026-05-02
