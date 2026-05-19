# dontyeet-network

**Layer:** Services | **Deps:** primitives

## Purpose
HTTP client abstraction for RPC and API calls, with privacy features.

## Modules

### `client` — HTTP abstraction
- `HttpClient` trait — async `get`/`post_json`
- `ReqwestClient` — concrete impl with configurable timeout + optional Tor/SOCKS5

### `jsonrpc` — JSON-RPC 2.0
- `RpcClient<C>` — wraps any `HttpClient`, handles request/response framing
- `call(method, params)` — single RPC call
- Used by EVM, Solana, XRP chain crates

### `retry` — Exponential backoff
- `RetryPolicy` — max retries, base delay, max delay
- `with_retry(policy, || async { ... })` — retries on timeout/429/5xx only

### `rate_limit` — Token bucket
- `TokenBucket` — capacity + refill rate, `try_acquire()` / `acquire().await`

### `cache` — TTL response cache
- `TtlCache<V>` — `get`/`set`/`invalidate`/`evict_expired`/`clear`

### `privacy` — Tor + endpoint rotation
- `PrivacyMode` — `Direct` (default) or `Proxy { proxy_url }` (Tor/SOCKS5)
- `EndpointRotator` — round-robin across multiple RPC providers
- Future: local node support (eliminates privacy problem entirely)

### `endpoints` — Per-network URL bag
- `Endpoints` — wraps `HashMap<NetworkId, Vec<Url>>`
- `primary(&network) -> Result<&Url>` — first URL or `NotFound`
- `all(&network) -> Result<&[Url]>` — full list for rotation/health checks
- `contains(&network) -> bool`
- Used by every chain crate's `broadcast.rs` / `balance.rs` to replace the
  4-line `urls.get(...).first().ok_or_else(...)` preamble.

### `network_catalog` — Catalog of supported networks
- `NetworkCatalog` — concrete impl of both `NetworkProvider` and
  `RpcEndpointProvider` from `dontyeet-primitives`.
- Constructed from owned `Vec<BlockchainNetwork>` + two `HashMap`s.
- Replaces the per-chain `BtcNetworkProvider` / `SolNetworkProvider` / ...
  structs that were byte-identical except for the type name.

## Key Traits
- `HttpClient` — async HTTP abstraction
