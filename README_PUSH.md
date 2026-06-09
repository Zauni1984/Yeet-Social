# Web Push for Yeet Social

The messaging layer can wake up the user's browser/PWA via the Web Push
API when they're offline (no WebSocket open). Pushes are **tickle**
pushes — no payload, no plaintext, no ciphertext. The service worker
renders a generic "New message" notification and the user opens the
app to see the actual messages.

## Patent / privacy stack

- Web Push (RFC 8030) + VAPID (RFC 8292) — open standards, royalty-free.
- ECDSA P-256 + SHA-256 for VAPID JWT signing — same curve family as
  the LiveKit token signing, no new patent surface.
- The empty payload sidesteps RFC 8291 (aes128gcm) so we don't carry
  any per-push encryption code; if we ever decide to put encrypted
  content in pushes the SW already has the `p256dh_key` and
  `auth_key` from the browser, but we don't ship that today.

## Generating VAPID keys

Once per deployment. Run on any host with openssl:

```bash
# 1) raw P-256 keypair
openssl ecparam -name prime256v1 -genkey -noout -out vapid_priv.pem
openssl ec -in vapid_priv.pem -pubout -out vapid_pub.pem

# 2) extract the raw 32-byte private scalar (base64url-no-pad)
openssl ec -in vapid_priv.pem -text -noout 2>/dev/null \
  | sed -n '/priv:/,/pub:/p' \
  | head -n -1 | tail -n +2 \
  | tr -d ' \n:' | xxd -r -p \
  | base64 | tr '+/' '-_' | tr -d '='

# 3) extract the raw 65-byte uncompressed public point (base64url-no-pad)
openssl ec -in vapid_priv.pem -text -noout 2>/dev/null \
  | sed -n '/pub:/,/ASN1 OID/p' \
  | head -n -1 | tail -n +2 \
  | tr -d ' \n:' | xxd -r -p \
  | base64 | tr '+/' '-_' | tr -d '='
```

The frontend's `applicationServerKey` is the **public** key from step 3.

## Server env

Set these in the backend deploy environment:

```
VAPID_PUBLIC_KEY=<base64url no-pad public key, ~88 chars>
VAPID_PRIVATE_KEY=<base64url no-pad private scalar, ~43 chars>
VAPID_SUBJECT=mailto:admin@justyeet.it
```

`VAPID_SUBJECT` defaults to `mailto:admin@justyeet.it` if unset.

Restart the backend. `GET /api/v1/push/config` should now return
`{"data": {"enabled": true, "vapid_public_key": "..."}}`.

If the env vars are missing, the endpoint reports `enabled: false`
and the settings UI shows "Push notifications are not configured on
the server yet". All other functionality is unaffected.

## Tickle flow

1. User opts in via Settings → Push notifications.
2. Browser prompts for permission. On grant, the SW registers and a
   PushSubscription is created with the VAPID public key.
3. Client posts `{endpoint, p256dh_key, auth_key, user_agent}` to
   `POST /api/v1/me/push/subscribe`.
4. Server stores the subscription in `push_subscriptions` (one row
   per browser/device).
5. When a new message arrives for a recipient who has **no active
   WebSocket**, `tickle_offline_recipients` fires a content-less HTTP
   POST to the push service with a signed VAPID `Authorization`
   header. The push service forwards a wake-up to the SW.
6. SW renders "New message" → click opens the app.

## What the server does NOT do

- Send any message content (or its existence count, or sender) in
  the push payload. Recipients who care about deeper UX can wire the
  client to fetch fresh state on SW activation.
- Implement RFC 8291 payload encryption. The code path is empty
  `Content-Length: 0`; the auth key on the subscription is stored
  for a future upgrade only.
- Tickle anyone who's currently online via WebSocket — that would
  double-notify.
