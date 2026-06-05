# LiveKit Deployment for YEET Social Phase 2

## Status: parked

The live-streaming feature is fully built end-to-end (UI, schedule,
tip-ranking, paid promotions, WebRTC ingest, viewer rendering) but
**deactivated in production** until Yeet hits the user base that
justifies running a self-hosted LiveKit cluster — roughly **several
million users**.

Reasoning:

- A LiveKit deployment that can handle thousands of concurrent
  publishers + tens of thousands of viewers needs dedicated hardware
  (CPU + outbound bandwidth) that's wasted on a smaller community.
- Until the audience is large enough that "people are live right
  now" is consistently true, an empty Live tab hurts the product
  more than a parked tab.
- Code stays in the repo so when we flip the flag we ship a feature
  that's already been reviewed, not a months-old design doc.

### What's deactivated, what isn't

| Component                               | State on prod                         |
|-----------------------------------------|---------------------------------------|
| Frontend LIVE tab                       | Hidden (commented-out in `index.html`) |
| Backend `/api/v1/lives/*` endpoints     | `403 LIVES_DEACTIVATED` if called     |
| `lives` / `live_promotions` tables      | Migrated, empty                       |
| Scheduled **posts** (separate feature)  | **Active** — unaffected               |
| YEET tipping on regular posts           | Active — unaffected                   |
| LiveKit server                          | Not deployed                          |

### Re-enabling (when we're ready)

1. Deploy LiveKit per the instructions below.
2. Set on the backend:
   ```
   LIVES_FEATURE_ENABLED=true
   LIVEKIT_WS_URL=wss://livekit.justyeet.it
   LIVEKIT_API_KEY=<API_KEY>
   LIVEKIT_API_SECRET=<API_SECRET>
   ```
3. In `frontend/index.html`, uncomment the LIVE tab in the tab bar
   (search for "LIVE tab parked until Yeet has the user base").
4. Restart the backend, redeploy the frontend.
5. `curl /api/v1/lives/config` should return
   `{ "livekit_configured": true, "feature_enabled": true }`.

Everything else — the data model, the auto-promo posts, the YEET tip
ranking, the host UI, the LiveKit token minting — is already in
place and tested.

---

## Phase 2 deployment (when re-enabling)

Phase 2 of the live-stream feature uses a self-hosted LiveKit server.
The backend already knows how to mint tokens and the frontend already
knows how to connect — you just need to deploy LiveKit and set three
environment variables.

## Why this stack

- **LiveKit** is Apache 2.0 licensed and uses **WebRTC + VP8/VP9 + Opus**
  end-to-end. All codecs are royalty-free; there are no MPEG-LA H.264
  fees and no third party sees the traffic. This is the patent-safe
  path the product was scoped against.
- LiveKit runs as a single Go binary, has no external dependencies
  beyond UDP/TURN, and scales to thousands of concurrent participants
  on commodity hardware.

## Quick start (Docker, on the same VPS)

1. Pick an API key + secret. Generate them with:

   ```bash
   openssl rand -hex 16   # API key
   openssl rand -hex 32   # API secret
   ```

2. Create `/opt/livekit/config.yaml`:

   ```yaml
   port: 7880
   bind_addresses:
     - ""
   rtc:
     tcp_port: 7881
     port_range_start: 50000
     port_range_end: 60000
     use_external_ip: true
   keys:
     <API_KEY_FROM_STEP_1>: <API_SECRET_FROM_STEP_1>
   ```

3. Open firewall ports: `7880/tcp` (signalling), `7881/tcp` (TURN/TCP),
   `50000-60000/udp` (RTP).

4. Run LiveKit:

   ```bash
   docker run -d --name livekit \
     --network host \
     -v /opt/livekit/config.yaml:/etc/livekit.yaml \
     livekit/livekit-server \
     --config /etc/livekit.yaml
   ```

5. Terminate TLS in front of `:7880` (nginx / Caddy). The browser must
   reach it as `wss://livekit.justyeet.it`.

6. Set on the YEET backend (`.env` or systemd unit):

   ```
   LIVEKIT_WS_URL=wss://livekit.justyeet.it
   LIVEKIT_API_KEY=<API_KEY>
   LIVEKIT_API_SECRET=<API_SECRET>
   ```

7. Restart the backend. `GET /api/v1/lives/config` should now return
   `{ "livekit_configured": true }`.

## What the YEET backend does

- `POST /api/v1/lives/:id/start` mints an HS256 JWT with the LiveKit
  `video` claim (`canPublish: true`) and stores
  `lives.livekit_room = "yeet-<live_id>"`.
- `POST /api/v1/lives/:id/viewer-token` mints a subscriber token
  (`canPublish: false`). Anonymous viewers get a random identity
  prefixed `anon-`.
- All token minting lives in `backend/src/services/livekit.rs`. If
  the env vars are missing, the endpoints reply
  `503 LIVE_NOT_CONFIGURED` and the frontend shows a clear setup hint.

## Production checklist

- [ ] LiveKit server listening on `:7880` behind a TLS reverse proxy.
- [ ] UDP `50000-60000` reachable from the public internet (no
      symmetric NAT issues; expose them on a public IP or run
      `coturn` alongside).
- [ ] `LIVEKIT_WS_URL` uses `wss://` (not `ws://`) — browsers refuse
      mixed-content WebSocket connections from `https://justyeet.it`.
- [ ] Backend has the three env vars and was restarted.
- [ ] `curl https://api.justyeet.it/api/v1/lives/config` returns
      `livekit_configured: true`.
