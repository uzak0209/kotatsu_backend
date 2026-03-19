# Kotatsu 2D Race/Battle Game Spec (v0.1)

## 1. Scope
- Platform: PC (Full HD / 16:9)
- Frontend: Unity 6000.3.11f1
- Networking: QUIC-based realtime transport (32ms tick target)
- This document defines **MVP** behavior for backend integration.

## 2. Game Flow
1. Title/Menu
   - Online
   - Solo (stage debug / single player)
   - Manual
   - Exit
2. In-game
   - Race/action gameplay
   - Pause (optional for MVP)
3. Result
   - Return to title after finish

## 3. Core Rules (MVP)
- Side-view 2D stage, goal-oriented race progression.
- Players can adjust gameplay parameters to help self or disturb opponents.
- Parameter effects are relative to each player's current location/situation, enabling strategic interference.

## 4. Controllable Parameters
Baseline:
- Gravity and Move Speed use 3-stage values (`LOW`, `MID`, `HIGH`)
- Friction uses 2-stage values (`OFF`, `ON`)

1. Gravity
- LOW: weak jump / heavy feel
- MID: default
- HIGH: strong jump / high head-hit risk

2. Move Speed
- LOW: slower but precise
- MID: default
- HIGH: faster but harder control

3. Friction
- OFF: slippery (ice-like)
- ON: default friction

## 5. Stage Design Constraints
- Each stage should expose at least 3 adjustable mechanics (strategic requirement).
- Stage blocks may emphasize different hazards (holes, spikes, straight paths, etc.).
- Status effects should stay meaningful even when players are on different stage segments.

## 6. Input Design (Backend-relevant abstraction)
Frontend may support Arrow/WASD/Gamepad variants, but backend treats actions abstractly:
- `MOVE`
- `JUMP`
- `PARAM_INC`
- `PARAM_DEC`
- `PARAM_SWITCH`
- `PARAM_APPLY`

Backend should not depend on physical key bindings.

## 7. Tick and Sync
- Authoritative server tick: 32ms target (31.25Hz).
- Client sends:
  - reliable parameter changes on QUIC stream
  - unreliable position updates on QUIC datagram
- Server returns state snapshots + event deltas.

## 8. Non-goals in MVP
- Mobile-specific control/UI
- Full replay system
- Full anti-cheat beyond basic authority checks
