# Local Latency Benchmark (2026-03-17)

## Goal
Compare local round-trip latency differences across transport/runtime options:
- TCP
- UDP
- WebSocket
- Node.js (TypeScript runtime equivalent)
- Rust

## Environment
- Machine: local dev machine (`127.0.0.1` loopback)
- Date: 2026-03-17
- Node: `v25.1.0`
- Rust: `rustc 1.91.1`
- Iterations: `3000` (warmup `300`)

## Method
- Echo benchmark (1 request -> 1 response RTT)
- Sequential request/response (no pipelining)
- Payload sizes:
  - 32B (small control message)
  - 512B (heavier state/event packet)

## Raw Result Snapshot

### Node (32B)
- TCP: `p50=0.026958ms`, `p95=0.086708ms`, `p99=0.122375ms`
- UDP: `p50=0.039959ms`, `p95=0.095500ms`, `p99=0.138333ms`
- WebSocket: `p50=0.068792ms`, `p95=0.217042ms`, `p99=0.427166ms`

### Rust (32B)
- TCP: `p50=0.034834ms`, `p95=0.059292ms`, `p99=0.074916ms`
- UDP: `p50=0.026292ms`, `p95=0.056625ms`, `p99=0.072834ms`

### Node (512B)
- TCP: `p50=0.100125ms`, `p95=2.770167ms`, `p99=10.399625ms`
- UDP: `p50=0.182625ms`, `p95=3.146125ms`, `p99=10.565791ms`
- WebSocket: `p50=0.118334ms`, `p95=1.549125ms`, `p99=4.716125ms`

### Rust (512B)
- TCP: `p50=0.190334ms`, `p95=3.577500ms`, `p99=8.126083ms`
- UDP: `p50=0.070792ms`, `p95=2.274542ms`, `p99=6.459375ms`

## Quick Read
- On localhost, all transports are sub-millisecond at p50.
- Tail latency (p95/p99) varies more than median; scheduler/runtime jitter dominates.
- For 32B control-like packets:
  - Node: TCP ~= UDP < WebSocket (median)
  - Rust: UDP ~= TCP (both very low)
- Language difference alone is not consistently dominant in this microbenchmark.

## Important Caveats
- This is **loopback** only (no real internet, no packet loss, no congestion).
- Results are sensitive to OS scheduler and power state.
- WebTransport is not included yet in this local run.

## How to Re-run
```bash
node benchmarks/latency-node.mjs
SIZE=512 node benchmarks/latency-node.mjs

rustc benchmarks/latency-rust.rs -O -o benchmarks/latency-rust
benchmarks/latency-rust
SIZE=512 benchmarks/latency-rust
```

## Next for WebTransport vs WebSocket
To compare fairly, add a QUIC/WebTransport echo server and run over:
1. localhost
2. LAN (same router)
3. WAN (Cloudflare/AWS edge path)

Only then WebTransport-specific gains/losses (HOL blocking avoidance, congestion behavior) become meaningful.
