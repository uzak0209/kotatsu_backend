# Pure QUIC vs WebSocket (32ms Interval, localhost)

Date: 2026-03-18 (JST)

## Condition
- Send interval: 32ms
- Requests: 300
- Payload: 32 bytes
- QUIC mode: datagram (pure QUIC)
- Environment: localhost (127.0.0.1)

## Script
- `benchmarks/quic_vs_ws/src/main.rs`
- Run:
```bash
cd benchmarks/quic_vs_ws
cargo run --release
```

## Results (mean RTT)
- Run 1: QUIC `0.212 ms`, WebSocket `0.434 ms`
- Run 2: QUIC `0.707 ms`, WebSocket `0.865 ms`
- Run 3: QUIC `0.320 ms`, WebSocket `0.476 ms`

## Aggregate
- QUIC mean-of-means: `0.413 ms`
- WebSocket mean-of-means: `0.592 ms`

## Takeaway
In this local benchmark, pure QUIC datagram responded faster on average than WebSocket under 32ms periodic send.
