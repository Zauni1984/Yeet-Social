# dontyeet-account — AI Reference

> **Layer:** Services | **Deps:** primitives, crypto, storage

## Purpose

Account lifecycle and session gate for DontYeetWallet. Composes
`EncryptedStore` (from `dontyeet-storage`), `Argon2Hasher` (from
`dontyeet-crypto`), and an in-process `Session` state machine to
provide the wallet's authenticated surface: create / login / logout
/ change password / delete / verify, plus typed-storage entry points
(`secure_set` / `secure_get` / `secure_delete`) that other crates use
to persist values encrypted under the unlocked session key without
ever holding the key themselves.

Phase trace: pre-M.5 the only consumer was `dontyeet-app` (server
process). M.5 moved consumption into `dontyeet-ui` (browser WASM); the
*crate* did not move, only its caller. Today both targets are
supported: the crate compiles for native (tests, dev tooling) and
`wasm32-unknown-unknown` (the production consumer) — `web-time` is
the cross-platform `Instant` shim that makes `Session`'s idle
tracking work in both.

## Module Map

```
src/
  lib.rs           Pub re-exports (AccountManager, Session, AccountError)
  manager.rs       AccountManager — the public 10-op API
  session.rs       Session state machine + DEFAULT_SESSION_TIMEOUT
  mnemonic_repo.rs MnemonicRepository — login-gated mnemonic CRUD
  error.rs         AccountError + AccountResult
```

## State Machine

```
                  create()                login()
[NoAccount] ───────────────► [Locked] ───────────────► [Unlocked]
                                ▲                          │
                                │                          │ logout()
                                │                          │ idle > timeout
                                └──────────────────────────┘
                                                           │
[NoAccount] ◄─── delete() / mark_deleted() ────────────────┘
```

Three states, four transitions. The encryption key only exists in
memory while in `Unlocked`; it is wiped via `Zeroize` on every
transition out (logout, idle-timeout, delete, drop). `web_time::Instant`
backs the idle clock so this works identically in WASM and native.

`Session::encryption_key()` is the single read-side gate: it auto-
locks the session if the timeout has elapsed (refusing to hand back
the key) and otherwise refreshes the activity stamp. Callers never
get raw `&[u8]` lifetime past the call site — `AccountManager`
copies into a `Zeroizing<Vec<u8>>` for each crypto op.

## AccountManager — public API

```rust
pub struct AccountManager<B: KeyValueBackend, C: Cipher> { /* ... */ }

// Construction
pub fn new(store: EncryptedStore<B, C>) -> Self;
pub async fn initialize(&self) -> AccountResult<()>;          // Sync session w/ storage at startup

// Lifecycle (6 ops)
pub async fn create(&self, mnemonic: &Mnemonic, password: &str) -> AccountResult<()>;
pub async fn login(&self, password: &str) -> AccountResult<()>;
pub fn logout(&self) -> AccountResult<()>;
pub async fn change_password(&self, current: &str, new: &str) -> AccountResult<()>;
pub async fn delete(&self, password: &str) -> AccountResult<()>;
pub async fn verify_password(&self, password: &str) -> AccountResult<()>; // Side-effect-free re-check

// Inspection
pub async fn exists(&self) -> AccountResult<bool>;
pub fn is_logged_in(&self) -> AccountResult<bool>;
pub async fn get_mnemonic(&self) -> AccountResult<Mnemonic>;

// Tier-2 typed storage entry points (consumed by dontyeet-ui::storage::secure)
pub async fn secure_set<T: Serialize>(&self, key: &str, value: &T) -> AccountResult<()>;
pub async fn secure_get<T: DeserializeOwned>(&self, key: &str) -> AccountResult<Option<T>>;
pub async fn secure_delete(&self, key: &str) -> AccountResult<()>;
```

Notes:

- `create` leaves the session **unlocked** with the freshly-derived
  key in memory. Callers must *not* immediately follow up with
  `login(...)` — that would re-run Argon2 unnecessarily.
- `secure_*` require an unlocked session. `secure_delete` requires
  it even though delete doesn't decrypt — refusing mutations from a
  locked wallet protects the encrypted namespace from corruption.
- All `secure_*` paths use `Zeroizing<Vec<u8>>` for the working key
  copy.

## Key derivation chain

Two Argon2 derivations per password, on different salts, producing
two distinct keys:

```
password ──► Argon2(bootstrap_salt) ──► bootstrap_key
                                          │
                                          ▼
              decrypts:  account:password_hash, account:key_salt

password ──► Argon2(key_salt)       ──► encryption_key
                                          │
                                          ▼
              decrypts:  account:mnemonic
                         + every Tier-2 entry written via secure_set
```

The bootstrap key exists so the password verification material
(`password_hash`) is itself encrypted at rest — no plaintext password
hash on disk. The bootstrap salt is per-installation and random for
v2 accounts; v1 accounts (created before the random-salt scheme)
fall back to a legacy fixed salt: `b"DontYeet-v1-boot"`. The legacy
fallback is read-only — `change_password` rotates to a new random
bootstrap salt — so v1 installs migrate transparently on any write.

## Storage keys

| Key | Encryption | Purpose |
|-----|------------|---------|
| `account:bootstrap_salt` | none (raw via `backend.set`) | Per-installation random salt for the bootstrap-key derivation. v1-account fallback returns `b"DontYeet-v1-boot"` if absent. |
| `account:password_hash` | bootstrap_key | Argon2 hash of the password (verification material). |
| `account:key_salt` | bootstrap_key | Random salt that mixes with the password to derive `encryption_key`. |
| `account:mnemonic` | encryption_key | The seed phrase. Read only when session is unlocked. |

Tier-2 callers (e.g. `dontyeet-ui::storage::secure::SecureKey::*`) write
under arbitrary string keys via `secure_set`, encrypted with
`encryption_key`.

## DEFAULT_SESSION_TIMEOUT

`session::DEFAULT_SESSION_TIMEOUT = 5 minutes` of inactivity. The UI
layer (`dontyeet-ui::idle`) layers a separate user-driven idle clock
on top — its default is 3 min and is configurable via Settings
(`AutoLockTimeout::ThreeMinutes` etc.). The two clocks are
independent: the UI clock locks first under default settings, but the
account-crate timeout is the floor that protects against hung pages
or stalled spawn_local tasks.

`Session::set_timeout(...)` lets the consumer override the default
(used by integration tests and could be used by a configurable UI
toggle).

## Errors

```rust
pub enum AccountError {
    NotAuthenticated,    // session locked or timed out
    AlreadyExists,       // create() with account present
    NotFound,            // login() / delete() with no account
    WrongPassword,       // password verify failed
    Storage(String),     // backend / serialization / encryption
    Crypto(String),      // hashing / key derivation
}
```

`From<StorageError>` and `From<CryptoError>` impls funnel underlying
errors into `Storage` / `Crypto` variants. There's also a
`From<AccountError> for DontYeetWalletError` impl that **collapses
`NotFound` and `WrongPassword` into the same public variant**
(`DontYeetWalletError::NotFound("account")`) to prevent account-enumeration
attacks via error-message differentiation.

## Concurrency

The `Session` is wrapped in a `std::sync::Mutex` inside
`AccountManager`. All public methods acquire the lock briefly
(release it before async storage ops where possible). The crate
itself is `Send + Sync`-friendly; the WASM single-thread model means
the lock is uncontended in production.

## Patterns

- **`secure_*` from a `RwSignal` event handler:** the UI's
  pattern is `wallet::account()` → `manager.secure_set(...)` inside a
  `spawn_local`. Reads on a locked session return
  `Err(NotAuthenticated)`; the UI populates Tier-2 signals via
  `state::populate_tier2` post-unlock.
- **Adding a new persisted value at the consumer level:** don't
  call this crate's API directly. Add a variant to the consumer's
  sealed key enum (`UiPref` or `SecureKey` in `dontyeet-ui`) and let
  that crate's typed wrapper route through `secure_set` / `secure_get`.
  Keeps the tier decision at the consumer's source-edit time.

## Tests

`cargo test -p dontyeet-account` runs an in-memory `MemBackend` impl
against `AccountManager` end-to-end (round-trip mnemonic, change_password,
delete, secure_* on locked vs unlocked session, etc.). 18 tests
across `manager.rs` and `session.rs`.
