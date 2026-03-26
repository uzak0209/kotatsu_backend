# realtime-split

Two-service architecture on the same host with Docker:
- `api-server` (HTTP/TCP matchmaking)
- `realtime-server` (QUIC realtime + gRPC control plane)

Internal communication:
- `api-server -> realtime-server` uses gRPC (`ControlPlane` service)

## Run with Docker
```bash
cd /Users/uzak/Projects/kotatsu/backend
cp .env.selfhost.example .env.selfhost
docker compose up --build
```

Quick API smoke test:
```bash
./smoke-test.sh
```

Generated OpenAPI:
```bash
curl -sS http://127.0.0.1:8080/openapi.json
```

Human-friendly API docs:
- `../docs/matchmaking-api.md`

Japanese QUIC protocol docs:
- `../docs/quic-realtime-protocol-ja.md`

4-client integration test (reliable + datagram paths):
```bash
./run-local-4clients.sh
```

Remote 4-client integration test against the self-hosted app (`kotatsu.ruxel.net` by default):
```bash
./run-remote-4clients.sh
```

From the repo root, you can also run:
```bash
just test-remote
```

The remote runner resolves the hostname first and uses the IP for API/QUIC access, which helps in environments where the test client cannot resolve the hostname reliably.

Remote QUIC datagram one-way latency measurement against the self-hosted app:
```bash
./measure-remote-rtt.sh
```

From the repo root:
```bash
just rtt-remote
```

This measures post-connect QUIC datagram one-way latency by connecting two clients to the same room and timing `client A -> server -> client B` on a shared local clock.

Prebuilt tester binaries for macOS, Windows, and Linux can be generated from the GitHub Actions workflow `Build Remote RTT Binaries`. Pushing a tag like `remote-rtt-v1.0.0` will also attach those binaries to a GitHub Release automatically. The packaged tester instructions live in `docs/remote-rtt-testers.txt`.

## Exposed ports
- `8080/tcp`: API server
- `4433/udp`: QUIC realtime
- `50051/tcp`: gRPC control plane (container-internal only, not router-opened)

## Router forwarding (self-host)
Forward only these from WAN to your host machine:
- `TCP 8080 -> <host_lan_ip>:8080` (matchmaking API)
- `UDP 4433 -> <host_lan_ip>:4433` (QUIC realtime)

Do not forward gRPC port `50051` to WAN.

## API usage
1. Create match
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/matches -H 'content-type: application/json' -d '{}'
```
2. Join match
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/matches/<match_id>/join -H 'content-type: application/json' -d '{"display_name":"p1"}'
```
3. Delete match
```bash
curl -sS -X DELETE http://127.0.0.1:8080/v1/matches/<match_id> -i
```

## Protocol split
- Reliable QUIC bidirectional stream:
  - Join auth
  - Gravity/Friction/Speed changes and broadcast
- Unreliable QUIC datagram:
  - 32ms position sync broadcast
