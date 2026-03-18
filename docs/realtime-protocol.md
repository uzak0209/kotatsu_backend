# Realtime Protocol Draft (WebTransport)

## 1. Client -> Server

### InputFrame
```json
{
  "t": "input",
  "seq": 124,
  "clientTimeMs": 1712345678901,
  "moveX": 1,
  "jump": false,
  "actions": ["PARAM_INC", "PARAM_APPLY"],
  "selectedParam": "gravity"
}
```

## 2. Server -> Client

### Snapshot
```json
{
  "t": "snapshot",
  "tick": 4021,
  "serverTimeMs": 1712345679010,
  "players": [
    {
      "playerId": "p1",
      "x": 14.2,
      "y": 3.8,
      "vx": 2.1,
      "vy": 0.0,
      "progress": 0.41,
      "params": { "gravity": "MID", "speed": "HIGH", "friction": "LOW" }
    }
  ],
  "events": [
    { "type": "PARAM_APPLIED", "playerId": "p1", "param": "speed", "value": "HIGH" }
  ]
}
```

### Result
```json
{
  "t": "result",
  "rankings": [
    { "playerId": "p2", "rank": 1, "goalTimeMs": 89210 },
    { "playerId": "p1", "rank": 2, "goalTimeMs": 90775 }
  ]
}
```

## 3. Reliability Strategy
- Inputs: unreliable datagram with sequence numbers.
- Critical events/result: reliable stream.
- Snapshot frequency target: every tick (or every 2 ticks under load).

## 4. Security
- All sessions bound to short-lived token from Control API.
- Reject stale `seq` window beyond tolerance.
- Rate-limit input bursts per player.
