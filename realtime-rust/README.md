# kotatsu realtime server (Rust)

Single process server containing:
- Matchmaking API (TCP/HTTP)
- Realtime server (QUIC)

This is one binary process with two listeners (API and QUIC).

## Ports
- API: `:8080`
- QUIC realtime: `:4433`

## Run
```bash
cd /Users/uzak/Projects/kotatsu/backend/realtime-rust
cargo run
```

## API
OpenAPI docs are auto-generated from Rust handlers:
- JSON: `GET /openapi.json`
- UI: `GET /docs`

### Create match
`POST /v1/matches`

Response:
```json
{ "match_id": "m_xxx", "max_players": 4 }
```

### Join match
`POST /v1/matches/:match_id/join`

Body:
```json
{ "display_name": "player1" }
```

Response:
```json
{
  "match_id": "m_xxx",
  "player_id": "p_xxx",
  "token": "...",
  "quic_url": "quic://0.0.0.0:4433",
  "token_expires_at_unix": 1234567890
}
```

## Realtime protocol
### Reliable stream (critical, must not drop)
Client first message must be:
```json
{ "t": "join", "token": "..." }
```

Parameter update:
```json
{ "t": "param_set", "seq": 10, "gravity": 1.2, "friction": 0.8, "speed": 1.1 }
```

Broadcast from server:
```json
{
  "t": "param_applied",
  "from_player_id": "p_xxx",
  "seq": 10,
  "params": { "gravity": 1.2, "friction": 0.8, "speed": 1.1 },
  "server_time_ms": 1710000000000
}
```

### Unreliable datagram (position @ 32ms)
Client datagram:
```json
{ "t": "pos", "seq": 200, "x": 1.2, "y": 3.4, "vx": 0.2, "vy": -0.1 }
```

Server datagram broadcast:
```json
{
  "t": "pos",
  "player_id": "p_xxx",
  "seq": 200,
  "x": 1.2,
  "y": 3.4,
  "vx": 0.2,
  "vy": -0.1,
  "server_time_ms": 1710000000000
}
```
