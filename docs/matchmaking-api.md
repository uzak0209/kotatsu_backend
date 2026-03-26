# Kotatsu Matchmaking API

Human-friendly reference for the split matchmaking API exposed by `realtime-split/api-server`.

## Base URL
- Local: `http://127.0.0.1:8080`
- Self-hosted example: `http://kotatsu.ruxel.net:8080`

Machine-readable OpenAPI is available at:
- `GET /openapi.json`

## Conventions
- All request and response bodies use JSON.
- Timestamps ending in `_unix` are Unix time in seconds.
- Error responses use the shape `{"error":"..."}`.
- Current room capacity is `4` players.

## Typical Flow
1. Create a match with `POST /v1/matches`
2. Optional: call `GET /v1/matches` to show the current lobby list
3. Give the returned `match_id` to players
4. Each player calls `POST /v1/matches/{match_id}/join`
5. Client uses the returned `quic_url` and `token` to connect to realtime QUIC
6. Optional: poll `GET /v1/matches/{match_id}` to inspect room state
7. Optional: if the host cancels the lobby, call `DELETE /v1/matches/{match_id}`

## Endpoints

### `GET /health`
Health check for the API server.

Example:
```bash
curl -sS http://127.0.0.1:8080/health
```

Response:
```json
{
  "ok": true
}
```

### `GET /openapi.json`
Returns the generated OpenAPI 3.1 spec as JSON.

Example:
```bash
curl -sS http://127.0.0.1:8080/openapi.json
```

### `POST /v1/matches`
Creates a new match room.

Request body:
```json
{}
```

Example:
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/matches \
  -H 'content-type: application/json' \
  -d '{}'
```

Success response:
```json
{
  "match_id": "m_0123456789abcdef0123456789abcdef",
  "max_players": 4
}
```

Possible responses:
- `200 OK`
- `502 Bad Gateway`: control plane error

### `GET /v1/matches`
Returns all current lobbies.

Example:
```bash
curl -sS http://127.0.0.1:8080/v1/matches
```

Success response:
```json
{
  "matches": [
    {
      "match_id": "m_0123456789abcdef0123456789abcdef",
      "max_players": 4,
      "player_count": 1,
      "players": [
        {
          "player_id": "p_0123456789abcdef0123456789abcdef",
          "display_name": "p1",
          "gravity": 2,
          "friction": 2,
          "speed": 2,
          "next_param_change_at_unix": 0
        }
      ]
    }
  ]
}
```

Notes:
- Empty result is `{"matches":[]}`
- The list includes empty lobbies that have been created but not joined yet
- `player_count` is the current number of joined players in the lobby

Possible responses:
- `200 OK`
- `502 Bad Gateway`: control plane error

### `GET /v1/matches/{match_id}`
Returns the current room snapshot.

Path parameters:
- `match_id`: match identifier returned by `POST /v1/matches`

Example:
```bash
curl -sS http://127.0.0.1:8080/v1/matches/<match_id>
```

Success response:
```json
{
  "match_id": "m_0123456789abcdef0123456789abcdef",
  "max_players": 4,
  "players": [
    {
      "player_id": "p_0123456789abcdef0123456789abcdef",
      "display_name": "p1",
      "gravity": 2,
      "friction": 2,
      "speed": 2,
      "next_param_change_at_unix": 0
    }
  ]
}
```

Field notes:
- `gravity`: current gravity level
- `friction`: current friction level
- `speed`: current speed level
- `next_param_change_at_unix`: next time parameter changes are allowed for that player

Possible responses:
- `200 OK`
- `404 Not Found`: `{"error":"match_not_found"}`
- `502 Bad Gateway`: control plane error

### `DELETE /v1/matches/{match_id}`
Deletes a match room and invalidates any unused join tickets for that room.

Path parameters:
- `match_id`: match identifier returned by `POST /v1/matches`

Example:
```bash
curl -sS -X DELETE -i http://127.0.0.1:8080/v1/matches/<match_id>
```

Success response:
- `204 No Content`

Possible responses:
- `204 No Content`
- `404 Not Found`: `{"error":"match_not_found"}`
- `502 Bad Gateway`: control plane error

### `POST /v1/matches/{match_id}/join`
Issues a realtime join ticket for one player.

Path parameters:
- `match_id`: match identifier returned by `POST /v1/matches`

Request body:
```json
{
  "display_name": "p1"
}
```

Notes:
- `display_name` is optional
- if omitted or blank, the server uses `"player"`

Example:
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/matches/<match_id>/join \
  -H 'content-type: application/json' \
  -d '{"display_name":"p1"}'
```

Success response:
```json
{
  "match_id": "m_0123456789abcdef0123456789abcdef",
  "player_id": "p_0123456789abcdef0123456789abcdef",
  "token": "01234567-89ab-cdef-0123-456789abcdef",
  "quic_url": "quic://kotatsu.ruxel.net:4433",
  "token_expires_at_unix": 1760000000
}
```

Field notes:
- `token`: join token for the realtime server
- `quic_url`: public QUIC endpoint the client should connect to
- `token_expires_at_unix`: token expiry time, currently issued for about 1 hour

Possible responses:
- `200 OK`
- `404 Not Found`: `{"error":"match_not_found"}`
- `409 Conflict`: `{"error":"match_full"}`
- `502 Bad Gateway`: control plane error

## Quick Start
Create a match:
```bash
MATCH_ID="$(
  curl -sS -X POST http://127.0.0.1:8080/v1/matches \
    -H 'content-type: application/json' \
    -d '{}' | jq -r '.match_id'
)"
echo "$MATCH_ID"
```

Join as player 1:
```bash
curl -sS -X POST "http://127.0.0.1:8080/v1/matches/${MATCH_ID}/join" \
  -H 'content-type: application/json' \
  -d '{"display_name":"p1"}'
```

Inspect the room:
```bash
curl -sS "http://127.0.0.1:8080/v1/matches/${MATCH_ID}"
```

Delete the room:
```bash
curl -sS -X DELETE -i "http://127.0.0.1:8080/v1/matches/${MATCH_ID}"
```
