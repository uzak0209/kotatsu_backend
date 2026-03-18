# kotatsu_backend

Rust-first backend for the Kotatsu 2D game prototype.

## Current architecture
- `realtime-rust/`
  - single-process Rust prototype for matchmaking API + QUIC realtime
- `realtime-split/`
  - split Rust services for the current direction
  - `api-server`: HTTP matchmaking API
  - `realtime-server`: QUIC realtime server
  - internal communication via gRPC

## Main docs
- `docs/game-spec.md`
- `docs/latency-benchmark.md`
- `docs/netem-quic-vs-ws.md`
- `docs/pure-quic-interval32.md`
- `docs/cloudflare-ddns-setup.md`

## Realtime split setup
```bash
cd /Users/uzak/Projects/kotatsu/backend/realtime-split
cp .env.selfhost.example .env.selfhost
docker compose up --build
```

## Validation
```bash
cd /Users/uzak/Projects/kotatsu/backend/realtime-split
cargo check
cargo test -p kotatsu-realtime-server-split
./run-local-4clients.sh
```
