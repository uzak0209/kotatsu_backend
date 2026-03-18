# kotatsu_backend

Kotatsu 2D game backend prototype (Cloudflare Worker + KV).

## Setup
1. Install dependencies
```bash
npm install
```
2. Update `wrangler.toml` KV IDs
3. Run local dev server
```bash
npm run dev
```

## API (MVP)
- `GET /health`
- `GET /v1/stages`
- `POST /v1/matches`
- `POST /v1/matches/:matchId/join`
- `POST /v1/matches/:matchId/start`
- `GET /v1/matches/:matchId`
- `POST /v1/matches/:matchId/params/apply`

## Docs
- `docs/game-spec.md` - game-level requirements summary
- `docs/backend-spec.md` - backend architecture and API rules
- `docs/realtime-protocol.md` - WebTransport message contract draft
- `docs/latency-benchmark.md` - local transport/runtime benchmark results
- `docs/cloudflare-ddns-setup.md` - Cloudflare DDNS setup (PC script)

## Benchmark Scripts
- `benchmarks/latency-node.mjs` - TCP/UDP/WebSocket RTT on Node
- `benchmarks/latency-rust.rs` - TCP/UDP RTT on Rust
- `scripts/cloudflare-ddns-update.sh` - update Cloudflare A/AAAA record on IP change

## Realtime Server (Rust)
- `realtime-rust/` - matchmaking API (TCP/HTTP) + QUIC realtime server

## Notes
- Current implementation uses in-memory fallback when `GAME_KV` is not bound.
- Realtime WebTransport gateway is documented but not implemented in this MVP.
