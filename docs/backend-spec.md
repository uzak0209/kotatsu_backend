# Backend Spec (v0.1)

## 1. Service Split
Backend is split into two logical services:

1. Control API (HTTP)
- Match lifecycle (create/join/start/end)
- Stage metadata
- Session issuance
- Result persistence hooks

2. Realtime Gateway (WebTransport)
- Input ingestion
- Server-authoritative simulation loop
- Snapshot/event broadcast

For MVP in this repository, we provide Control API + protocol contracts.

## 2. Data Model (MVP)

### Match
- `matchId: string`
- `stageId: string`
- `status: WAITING | RUNNING | FINISHED`
- `createdAt: string`
- `seed: number`
- `maxPlayers: number`
- `players: PlayerState[]`

### PlayerState
- `playerId: string`
- `displayName: string`
- `connected: boolean`
- `progress: number` (0.0 - 1.0)
- `params: { gravity: Level3; speed: Level3; friction: Level3 }`

### StageConfig
- `stageId: string`
- `name: string`
- `enabledParams: ParamType[]`

### Enums
- `Level3 = LOW | MID | HIGH`
- `ParamType = gravity | speed | friction`

## 3. API Endpoints (MVP)
- `GET /health`
- `GET /v1/stages`
- `POST /v1/matches`
- `POST /v1/matches/:matchId/join`
- `POST /v1/matches/:matchId/start`
- `GET /v1/matches/:matchId`
- `POST /v1/matches/:matchId/params/apply`

## 4. KV Layout (Cloudflare KV)
- `match:{matchId}` -> Match JSON
- `stage:{stageId}` -> StageConfig JSON
- `player-session:{token}` -> `{ matchId, playerId, expiresAt }`

## 5. Validation Rules
- No join when match status != `WAITING`
- Max player cap hard-enforced
- Param apply allowed only for enabled stage params
- Level3 transitions are clamped (LOW/MID/HIGH)

## 6. WebTransport Contract (next phase)
- Control API issues signed short-lived session token.
- Realtime Gateway validates token and binds player to match.
- Gateway owns physics authority and resolves conflicts.

## 7. Open Questions / Consultation Items
1. Parameter scope: global-per-player or stage-zone-scoped at runtime?
2. Friction HIGH final behavior: sticky wall vs strong ground stop.
3. Matchmaking: invite code only vs quick match queue.
4. Result policy: rank by goal time only or include interference score.
5. Infra decision gate:
   - Cloudflare path: TS Worker + Durable Objects + KV
   - AWS path: Rust (QUIC/WebTransport stack) + managed DB
