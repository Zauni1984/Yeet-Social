# 18+ Age Verification

Real human review of an opt-in face scan + optional government ID,
replacing the previous self-declared `users.age_verified_at` flag. On
approval the user gets a permanent **purple 18+ badge** that can be
hidden (without revoking the verification itself) via Settings.

## Flow

1. User visits Settings → Age verification, or clicks the 18+ nav link.
2. Modal captures a face frame from the live camera (fallback:
   `<input type="file" capture="user">`).
3. After a non-black sanity check the modal reveals an optional
   ID-document picker (passport / driver's license / national ID / other).
4. **Submit** posts a multipart form to
   `POST /api/v1/me/age-verification/submit` (face_scan required,
   id_document + id_type optional).
5. Backend encrypts each blob with AES-256-GCM under a per-blob key
   derived via HKDF-SHA256 from `AGE_VERIFY_KEY` (info: case id + slot),
   writes them under `PRIVATE_DIR` (default `/app/private/...`), and
   inserts an `age_verification_cases` row with `status='pending'`.
6. Admin queue at **Admin → 18+ Queue** lists pending cases. Each case
   has "View face" / "View ID" buttons that fetch + decrypt the blob
   into a sandboxed popup. Approve/Reject prompts for a decision note
   shown back to the user; both actions are logged to `admin_actions`.
7. On approval, `users.age_verified_at` is set; the badge appears on
   the user's profile.
8. The background cleanup job purges blobs:
   - 7 days after approval, or
   - 30 days after rejection (appeal window),
   - immediately on withdraw.
   Purge = overwrite-with-zeros then unlink; metadata is kept for the
   audit trail and the `blobs_purged_at` column is set.

## Required environment

```
PRIVATE_DIR=/app/private          # default; create + chmod 700
AGE_VERIFY_KEY=<base64(32 bytes)> # mandatory; submit endpoint refuses if unset
ADMIN_SECRET=<≥24 chars>          # already required for the rest of /admin
```

Generate `AGE_VERIFY_KEY` once and persist it; rotating it
invalidates every existing encrypted blob.

```
openssl rand -base64 32
```

Mount `/app/private` as a docker volume separate from `/app/uploads`
so a misconfigured nginx location can't accidentally serve it:

```yaml
volumes:
  - /var/lib/yeet/private:/app/private
```

Confirm nginx has **no** `location /private/` and no `ServeDir` over
that path. The only way out for these blobs is the
`/api/v1/admin/age-verification/:case_id/blob` endpoint, which
requires the admin secret and is rate-limited by the general admin
posture.

## Hiding the badge

A verified user can flip the purple badge off in Settings → Age
verification (toggle "Show 18+ badge on my profile"). This sets
`users.age_badge_hidden` only — the verification itself is untouched,
the user can flip it back on at any time, and 18+ content is still
unlocked for them.

## Revocation

Admins can strip a verification after the fact via
`POST /api/v1/admin/users/:address/age-verify/revoke` (audited as
`age_verify_revoke`). Useful when a previously-approved decision
turns out to be wrong.

## What the server never sees in plaintext

- Face frame and ID document never land in `users.*` rows or on a
  publicly-served path.
- The encryption AAD binds (case_id, slot), so a stolen ciphertext
  can't be replayed as a different slot or moved between cases.
- Once the grace window has passed, the on-disk file is overwritten
  with zeros before unlinking so a later raw-block recovery yields
  nothing.
