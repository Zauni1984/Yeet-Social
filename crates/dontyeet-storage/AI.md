# dontyeet-storage

**Layer:** Core | **Deps:** primitives, crypto

## Purpose
Encrypted key-value storage abstraction. Backend-agnostic — implementations injected at runtime.

## Modules

### `backend` — Raw storage trait
- `KeyValueBackend` — async `get`/`set`/`delete`/`list_keys`/`clear` (raw bytes)
- Implementations provided by the app layer (filesystem, `SQLite`, browser, etc.)
- Returned futures are `Send` on native targets and non-`Send` on `wasm32-unknown-unknown` (`web_sys` types are single-threaded). Gated via `cfg_attr(async_trait)` / `cfg_attr(async_trait(?Send))`.

### `encrypted` — Encrypted wrapper
- `EncryptedStore<B, C>` — wraps any `KeyValueBackend` + any `Cipher`
- Encrypts on write, decrypts on read; tamper detection via AES-GCM
- **Fixed 512-byte container** with a 1-byte version prefix protected
  by the AES-GCM auth tag — flipping the version byte trips
  `Encryption(...)` rather than `UnknownVersion(...)`, distinguishing
  real corruption from a legitimate forward-version blob
- Container layout (post-decrypt): `[1-byte version][4-byte BE length][data][random padding to 512]`
- `pub const CURRENT_VERSION: u8 = 0x01` — current writer
- `pub const UNKNOWN_VERSION: u8 = 0x00` — reserved sentinel; never written
- `StorageError::UnknownVersion(u8)` — public error variant for forward-version reads
- `EncryptedStore::backend()` — escape hatch returning the raw `&B` for
  metadata that must be stored before the encryption key is available
  (used by `dontyeet-account` for the bootstrap salt)
- `EncryptedStore::get_versioned(...)` — variant of `get` that also
  returns the container's version byte for diagnostics / migration
- Max plaintext size: 507 bytes (`CONTAINER_SIZE - HEADER_SIZE`).
  Current callers (mnemonics ≤ 216 bytes, salts, password hashes,
  small JSON blobs) sit far below the limit.

### `serializer` — Typed serialization
- `Serializer` trait — `serialize<T>` / `deserialize<T>` over bytes
- `JsonSerializer` — default JSON implementation

## Key Traits
- `KeyValueBackend` — async raw byte-level storage
- `Serializer` — typed serialization
