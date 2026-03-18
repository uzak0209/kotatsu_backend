# 32ms Interval Benchmark: WebTransport vs WebSocket (Local)

Measured on 2026-03-18 (JST), localhost `127.0.0.1`.

## What was measured
- Send one request every `32ms`
- Total requests per run: `300` (about 9.6s)
- Payload: `32 bytes`
- Metric: response RTT (`send -> echo received`)

## Important environment note
- HTTP/3 (QUIC) WebTransport handshake failed in this environment due library/runtime compatibility.
- Therefore this benchmark uses **WebTransport over HTTP/2 (forceReliable)** for the WebTransport side.

## Commands
```bash
node benchmarks/interval32-ws-vs-webtransport.mjs
```

## Run Results

### Run 1
- WebSocket mean: `0.632 ms`
- WebTransport mean: `0.930 ms`

### Run 2
- WebSocket mean: `1.563 ms`
- WebTransport mean: `4.148 ms`

### Run 3
- WebSocket mean: `0.845 ms`
- WebTransport mean: `1.338 ms`

## Aggregate summary (3 runs)
- WebSocket mean-of-means: `1.014 ms`
- WebTransport mean-of-means: `2.139 ms`
- WebSocket median-of-means: `0.845 ms`
- WebTransport median-of-means: `1.338 ms`

## Interpretation
- In this local setup, WebSocket responded faster on average.
- There is noticeable jitter/outlier behavior (especially Run 2 on WebTransport).
- For game-quality decisions, add WAN/LAN tests and (ideally) HTTP/3 WebTransport measurements.
