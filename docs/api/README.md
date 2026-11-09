# YEET Social developer API

*Deutsch: [README.de.md](README.de.md) · Live page: https://justyeet.it/api.html · Spec: https://justyeet.it/openapi.json*

The API is the same HTTPS/JSON interface the Yeet app uses, at `https://justyeet.it/api/v1/`.
It is open to **autoposters, AI agents and other integrations** once the account has registered
and our team has **approved** its key.

## 1. Access model

| Step | What happens |
| --- | --- |
| Register | Any account on justyeet.it with a confirmed e-mail may apply. Posts made through the API appear under that account. |
| Apply | On https://justyeet.it/api.html (section *My keys*) while signed in: name, purpose (≥ 20 characters), kind (`autoposter` / `ai` / `other`), scopes. The secret `yeet_sk_…` is shown **once**; only its SHA-256 is stored. |
| Approval | The key is `pending` and answers `401 API key is pending` until an admin approves it in admin.html (tab *API keys*). Admins can set per-key limits and leave a note the applicant sees. |
| Use | `Authorization: Bearer yeet_sk_…`. The key acts as the owner, limited to its scopes and rate limits. |
| Revoke | Owner (api.html) or admin; revocation is immediate and final. Max. 5 non-revoked keys per account. |

## 2. Scopes

| Scope | Allows |
| --- | --- |
| `read` | feeds, posts, comments, profiles, search, notifications, public config |
| `write:posts` | create/delete own posts, visibility, media upload, scheduled posts |
| `write:interactions` | like, unlike, reshare, comment, follow/unfollow, mark notifications read |

Everything else is **never** reachable with a key: tips, pay-per-view unlock, points conversion,
paper wallets, messaging, settings, sessions, key management, admin. The allow-list is
`route_allowed()` in `backend/src/api/api_keys.rs`; anything not listed → `403 FORBIDDEN`.

## 3. Limits

- Per key: `rate_per_minute` (default 60) and `rate_per_day` (default 2 000) → `429 RATE_LIMITED`.
- Account rules apply unchanged: 420-character posts, daily points cap, posting bans, media caps
  (images 8 MB, video 32 MB/60 s, audio 80 MB/60 min, signature-checked).

## 4. Transparency

Every post created through a key is stored with `api_key_id` and `automation_kind` (= key kind)
and rendered with an **AUTO** or **AI** badge next to the author in the app and in
`GET /feed` responses (`automation_kind`). Applicants accept this when applying.

## 5. Endpoints (relative to `/api/v1`)

| Method | Path | Scope |
| --- | --- | --- |
| GET | `/me/api-keys/self` | — (whoami for the key) |
| GET | `/feed`, `/feed/following`, `/feed/permanent` | read |
| GET | `/posts/{id}`, `/posts/{id}/comments` | read |
| GET | `/users/me`, `/users/{id|@name|0x…}`, `…/posts`, `…/followers`, `…/following` | read |
| GET | `/search?q=`, `/notifications`, `/notifications/unread-count` | read |
| GET | `/boards`, `/lives/active`, `/points/rate`, `/tokens/pool`, `/config` | read |
| POST | `/posts` · DELETE `/posts/{id}` · PATCH `/posts/{id}/visibility` | write:posts |
| POST | `/uploads/post-media` (multipart `file`) | write:posts |
| POST | `/scheduled-posts` · GET `/scheduled-posts/mine` · DELETE `/scheduled-posts/{id}` | write:posts |
| POST | `/posts/{id}/like`, `/unlike`, `/reshare`, `/comments` | write:interactions |
| POST | `/users/{…}/follow`, `/unfollow`, `/notifications/read`, `/notifications/{id}/read` | write:interactions |

Key management (session JWT only, never with a key): `GET/POST /me/api-keys`,
`DELETE /me/api-keys/{id}`. Admin: `GET /admin/api-keys?status=`, `POST /admin/api-keys/{id}/approve|reject|revoke`.

## 6. Examples

```bash
curl -s https://justyeet.it/api/v1/me/api-keys/self -H "Authorization: Bearer $YEET_API_KEY"
curl -s https://justyeet.it/api/v1/feed?per_page=20 -H "Authorization: Bearer $YEET_API_KEY"
curl -s -X POST https://justyeet.it/api/v1/posts -H "Authorization: Bearer $YEET_API_KEY" \
  -H "Content-Type: application/json" -d '{"content":"Hello from my bot 👋","is_permanent":false}'
```

Response envelope: `{"success":true,"data":…}` or `{"success":false,"error":{"code":"…","message":"…"}}`.

## 7. Operations

- Approve/reject/revoke in admin.html → *API keys*; every decision is logged in `admin_actions`
  (`api_key_approve|reject|revoke`).
- Tables: `api_keys` (migration 0055), `posts.api_key_id`, `posts.automation_kind`.
- Rate limits live in Redis (`api_key` scope); revocation takes effect on the next request.
