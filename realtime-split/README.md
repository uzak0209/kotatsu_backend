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

4-client integration test (reliable + datagram paths):
```bash
./run-local-4clients.sh
```

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

## Protocol split
- Reliable QUIC bidirectional stream:
  - Join auth
  - Gravity/Friction/Speed changes and broadcast
- Unreliable QUIC datagram:
  - 32ms position sync broadcast
