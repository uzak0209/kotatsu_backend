# kotatsu_backend

Rust-first backend for the Kotatsu 2D game prototype.

## Current architecture
- `realtime-split/`
  - current backend source
  - `api-server`: HTTP matchmaking API
  - `realtime-server`: UDP realtime server
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

Optional host tuning for self-host deploys:
```bash
cp .sysctl.selfhost.example .sysctl.selfhost
```

## Main docs
- `docs/game-spec.md`
- `docs/latency-benchmark.md`
- `docs/netem-quic-vs-ws.md`
- `docs/pure-quic-interval32.md`
- `docs/cloudflare-ddns-setup.md`
- `docs/udp-realtime-protocol-ja.md`
- `docs/infra/README.md`

## Infrastructure diagrams
Generate the current self-hosted infra diagrams with `python-diagrams`:
```bash
uv run --with-requirements docs/infra/requirements.txt python3 docs/infra/generate_diagrams.py
```

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
- optional: `HOME_SERVER_SYSCTL`
- optional: `HOME_SERVER_SSH_PORT`
- optional: `HOME_SERVER_APP_DIR`

If `.sysctl.selfhost` exists locally, or `HOME_SERVER_SYSCTL` is set in GitHub Actions, deploy copies that content to `/etc/sysctl.d/99-kotatsu.conf` on the server and applies it immediately.

On the Alpine home server, the deploy script uses `podman` directly instead of Compose, installs an OpenRC boot hook when it can elevate with `doas` or `sudo`, and recreates the containers during deploy. That makes the host sysctl settings persistent and brings the backend containers back after reboot.
