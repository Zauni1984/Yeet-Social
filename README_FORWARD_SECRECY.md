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

## Phase 2 — SHIPPED (X3DH + Double Ratchet, 1:1 DM text)

The ratchet is now live for 1:1 DM **text** messages, gated and
fail-closed. Scope and safeguards:

- **Scope:** 1:1 DMs only (groups keep the per-member envelope
  scheme); text only (tips/images stay on the static path).
- **Capability-gated:** a message is sent forward-secret only when the
  peer has a prekey bundle *and* the sender has a master key to persist
  ratchet state. Otherwise it transparently falls back to the existing
  static ECDH+HKDF path.
- **Wire marker:** ratchet messages set `iv = "r2"`; the `ciphertext`
  field carries `base64(JSON({header, body}))`. The backend treats both
  as opaque, so no server change was needed — the bundle/prekey
  endpoints from phase 1 are reused as-is.
- **Fail-closed:** any ratchet error renders an "undecryptable"
  placeholder, never a crash, and never weakens to plaintext. The
  static path is untouched for everyone else.
- **Atomic decrypt:** decryption runs on a cloned state and commits
  only on AEAD success, so a tampered / duplicated / reordered message
  cannot advance and corrupt the receive chain.

### Verified before shipping

The core crypto (`/tmp/ratchet.mjs` during development) was exercised
with real WebCrypto test vectors in node — 13/13 passing:
X3DH establishment, DH ratchet on reply, out-of-order + skipped-key
delivery, 6 interleaved round-trips, AEAD tamper rejection, the
no-one-time-prekey path, and (critically) "state intact after tamper"
and "state intact after replay". The module inlined into `index.html`
is the byte-for-byte algorithm those tests cover.

## Phase 3 — SHIPPED (multi-device)

Prekeys, signing keys and ratchet sessions are now **per device**,
while the ECDH identity key stays shared across a user's devices
(it remains recoverable via the wallet/password master — the app's
existing identity model). Model: one identity, many devices.

- **Server (migration 0034):** `user_devices` table; `signed_prekeys`
  and `one_time_prekeys` gain `device_id`; uniqueness/indexes are now
  per `(user_id, device_id)`. `GET /users/:id/e2ee/bundles` returns
  one bundle per device, each atomically consuming that device's
  one-time prekey. `prekeys/count` and the upload are device-scoped.
- **Client:** a stable per-browser `device_id` is generated and sent
  with every prekey upload, so a second device never clobbers the
  first. Sending fans the message out to **every recipient device**
  plus the sender's **own other devices** (for cross-device sync);
  the wire is `{v:3, from:<senderDeviceId>, msgs:{<deviceId>:{header,
  body}}}` (still `iv:"r2"`). Each device decrypts its own entry,
  keying sessions by the remote device id.
- **Own sent messages:** the sending device isn't in its own fan-out,
  so it caches the plaintext locally (keyed by message id) to render
  its own bubble; other devices of the sender decrypt the self-sync
  copy.

Verified with node test vectors (19/19): adds B1+B2 fan-out, sender
self-sync to its other device, and cross-device entry rejection on top
of the phase-2 single-device suite. The inlined core was diffed
against the reference (identical KDF labels/constants, X3DH constant,
HKDF sizes, DH ordering).

### Remaining hardening (future)

- Ratchet for tips/images (text-only today).
- Glare (both peers initiate at once): resolved today by fail-closed +
  re-establish on next inbound preamble; a deterministic tiebreak would
  avoid the transient undecryptable message.
- A device-management UI (list/revoke devices) — `user_devices` has a
  `label` column reserved for it.
- Stale-device pruning: a device that never comes back keeps a (dead)
  bundle; senders waste one fan-out slot on it until its OTPs run out.
- Cross-check the KDF labels/constants against libsignal test vectors.

## What phase 2 did (the ratchet) — reference

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
