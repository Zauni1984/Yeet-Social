# dontyeet-crypto

**Layer:** Core | **Deps:** primitives

## Purpose
All offline cryptographic operations. **ZERO network or I/O dependencies.**

## Modules

### `mnemonic` — Seed generation
- `Bip39Generator` — BIP-39 mnemonic generation (12/24 words via `WordCount`), validation, mnemonic-to-seed

### `derivation` — HD key derivation
- `Bip44Deriver` — BIP-44 HD key derivation from seed + derivation path → `PrivateKey`
- `paths` submodule — well-known derivation path constants (Bitcoin, Ethereum, etc.)

### `cipher` — Encryption at rest
- `Cipher` trait — encrypt/decrypt bytes
- `CipherAlgorithm` enum — algorithm tag stored alongside ciphertext
- `AesGcmCipher` — AES-256-GCM authenticated encryption (quantum-safe symmetric)
- `HybridCipher` — ML-KEM (Kyber) key encapsulation + AES-256-GCM (post-quantum hybrid)

### `hasher` — Password hashing + KDF
- `PasswordHasher` trait — hash/verify passwords + `derive_key` for KDF
- `Argon2Hasher` — Argon2id with constant-time verification (quantum-safe)
- `Argon2Config` — exposed for callers that want non-default cost parameters

### `payload` — Encrypted data structures
- `EncryptedPayload` — algorithm + nonce + ciphertext (serializable)

## Security Guarantees
- All secret types implement `Zeroize + ZeroizeOnDrop`
- Password verification uses constant-time comparison (`subtle`)
- AES-256-GCM provides authenticated encryption (tamper detection)
- ML-KEM hybrid provides post-quantum protection for stored secrets
- **No** tokio, reqwest, hyper, std::net, std::fs in dependency tree
