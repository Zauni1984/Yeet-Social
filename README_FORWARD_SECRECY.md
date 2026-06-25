# Forward Secrecy — phase 1 (prekey foundation)

This is the first of two phases toward per-session forward secrecy for
Yeet DMs. **Phase 1 (this commit) ships the prekey infrastructure but
does not yet change how messages are encrypted.** The actual ratchet
that consumes these bundles is phase 2.

## Why two phases

The current DM crypto derives one static conversation key from
`ECDH(identity_a, identity_b)` + HKDF. It's server-blind and works, but
because the key never changes, a future compromise of an identity
private key exposes *all* past messages in that conversation. Forward
secrecy fixes that by giving every session (and, with a full ratchet,
every message) its own ephemeral key.

A correct Double Ratchet is ~1000 lines of carefully-tested crypto on
both ends; a subtle bug silently weakens confidentiality and passes
every compiler/linter. So we split the work: lay the safe, additive
foundation first (this phase), then build the ratchet on top as a
separately-reviewed effort.

## What phase 1 adds

### Keys (Signal model)
- **Signing identity key** — a dedicated ECDSA P-256 keypair. A
  WebCrypto ECDH key can't produce signatures, so the existing ECDH
  identity key can't sign prekeys; this separate key does. Public half
  stored in `users.e2ee_signing_public_key`, shipped in the bundle.
- **Signed prekey** — a medium-lived ECDH P-256 key whose public SPKI
  is signed by the signing identity key. One active per user, rotatable.
- **One-time prekeys** — single-use ECDH P-256 keys. The bundle
  endpoint hands out and atomically consumes one per fetch
  (`FOR UPDATE SKIP LOCKED`); the client replenishes when the pool
  runs low.

All private halves are wrapped under the same wallet/password-derived
master key as the identity key and stored locally; only public halves
leave the device. The server stays fully blind.

### Backend (migration 0033)
- `signed_prekeys`, `one_time_prekeys` tables + `users.e2ee_signing_public_key`.
- `POST /api/v1/me/e2ee/prekeys` — upload/rotate signing key + signed
  prekey + a batch of one-time prekeys (idempotent on retry).
- `GET /api/v1/me/e2ee/prekeys/count` — remaining one-time prekeys +
  whether a signed prekey exists, so the client knows when to top up.
- `GET /api/v1/users/:address/e2ee/bundle` — a recipient's bundle
  (identity key, signing identity key, signed prekey + signature, one
  claimed one-time prekey) for establishing a forward-secret session.
  Returns the OTP as null when the pool is exhausted (X3DH degrades
  safely with one fewer DH).

### Frontend
- `YeetE2EE` provisions the signing key, signed prekey, and 30 one-time
  prekeys at identity setup/recovery, reusing the master key already in
  scope (no extra prompt).
- `checkAndReplenishPrekeys()` tops the pool up opportunistically when
  the user is in messaging and the server count is low.
- Wrapped prekey privates are stored in `localStorage` under
  `yeet_prekeys_<user_id>` and wiped on logout.

## What phase 2 must do (the ratchet)

1. **X3DH handshake** — the sender fetches the recipient's bundle,
   verifies `signed_prekey.signature` against `signing_identity_key`,
   then computes the X3DH shared secret from
   `DH(IK_a, SPK_b) ‖ DH(EK_a, IK_b) ‖ DH(EK_a, SPK_b) ‖ DH(EK_a, OPK_b)`
   (last term omitted if no OTP).
2. **Double Ratchet** — seed the root key from the X3DH output; derive
   per-message keys via the symmetric-key ratchet, and rotate the DH
   ratchet on each round-trip. Persist per-conversation ratchet state
   (root key, chain keys, skipped-message keys) wrapped under the
   master key.
3. **Wire format** — messages carry the sender's current ratchet
   public key + message number in a header alongside the existing
   `ciphertext` + `iv`. The `messages` table already treats both as
   opaque, so no server change is required beyond perhaps a header
   column.
4. **Migration** — existing conversations keep the static-key scheme
   until both parties have ratchet state; new sessions use the ratchet.
   Detect capability via the presence of a bundle.

Phase 2 is intentionally not started here — it needs its own design
review and test vectors (ideally cross-checked against libsignal's)
before it touches message confidentiality.
