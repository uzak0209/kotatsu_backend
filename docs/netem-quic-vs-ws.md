# QUIC vs WebSocket with Linux netem (network-layer emulation)

Date: 2026-03-18 (JST)

## Why this method
Instead of app-level sleep, this benchmark injects delay/jitter/loss at network layer using `tc netem`.

## How it runs
- Linux Docker container (`rust:1.91-bookworm`)
- `tc qdisc add dev lo root netem ...`
- Benchmark binary: `benchmarks/quic_vs_ws`
- Traffic: 32ms interval, 300 requests, 32B payload

## Script
- `benchmarks/run-netem-quic-ws.sh`

## Results

### Scenario A: delay 0ms / jitter 0ms / loss 0%
- QUIC datagram mean RTT: `1.15 ms`
- WebSocket mean RTT: `0.53 ms`

### Scenario B: delay 20ms / jitter 2ms / loss 0%
- QUIC datagram mean RTT: `43.86 ms`
- WebSocket mean RTT: `115.94 ms`

### Scenario C: delay 40ms / jitter 4ms / loss 0%
- QUIC datagram mean RTT: `83.16 ms` (received 299/300)
- WebSocket mean RTT: `213.88 ms`

### Scenario D: delay 20ms / jitter 2ms / loss 0.5%
- QUIC datagram mean RTT: `44.38 ms` (received 297/300)
- WebSocket mean RTT: `125.13 ms`

## Notes
- QUIC is measured with **datagram mode** (unreliable); packet loss appears as receive loss.
- WebSocket runs on TCP; packet loss is hidden by retransmission, but RTT tail/mean can grow.
- These are emulated values, not real Cloudflare POP route measurements.
