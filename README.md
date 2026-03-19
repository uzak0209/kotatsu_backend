# kotatsu_backend

Rust-first backend for the Kotatsu 2D game prototype.

## Current architecture
- `realtime-split/`
  - current backend source
  - `api-server`: HTTP matchmaking API
  - `realtime-server`: QUIC realtime server
  - internal communication via gRPC

## Docker
Top-level Docker entrypoints:
- `docker-compose.yml`
- `Dockerfile.api`
- `Dockerfile.realtime`

Run locally:
```bash
cp .env.selfhost.example .env.selfhost
docker compose up --build
```

## Main docs
- `docs/game-spec.md`
- `docs/latency-benchmark.md`
- `docs/netem-quic-vs-ws.md`
- `docs/pure-quic-interval32.md`
- `docs/cloudflare-ddns-setup.md`

## Validation
```bash
cargo check --manifest-path realtime-split/Cargo.toml
cargo test --manifest-path realtime-split/Cargo.toml
docker compose config
```

## Deploy
```bash
just deploy-home <host> <user>
```

The default remote app dir is `/home/<user>/kotatsu-backend`.

GitHub Actions also supports deploy-on-push to `main` with these secrets:
- `HOME_SERVER_HOST`
- `HOME_SERVER_USER`
- `HOME_SERVER_SSH_KEY`
- `HOME_SERVER_ENV`
- optional: `HOME_SERVER_SSH_PORT`
- optional: `HOME_SERVER_APP_DIR`

On the Alpine home server, the deploy script uses `podman build` and `podman run --network host` directly instead of Compose.
