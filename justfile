set shell := ["zsh", "-cu"]

default:
  @just --list

check:
  cargo check --manifest-path realtime-split/Cargo.toml

test:
  cargo test --manifest-path realtime-split/Cargo.toml

test-remote host="kotatsu.ruxel.net" remote_ip="" api_port="8080" quic_port="4433" tick_ms="32" ticks="90":
  REMOTE_HOST="{{host}}" REMOTE_IP="{{remote_ip}}" API_PORT="{{api_port}}" QUIC_PORT="{{quic_port}}" TICK_MS="{{tick_ms}}" TICKS="{{ticks}}" ./realtime-split/run-remote-4clients.sh

rtt-remote host="kotatsu.ruxel.net" remote_ip="" api_port="8080" quic_port="4433" rtt_samples="10":
  REMOTE_HOST="{{host}}" REMOTE_IP="{{remote_ip}}" API_PORT="{{api_port}}" QUIC_PORT="{{quic_port}}" RTT_SAMPLES="{{rtt_samples}}" ./realtime-split/measure-remote-rtt.sh

up:
  docker compose up --build -d

down:
  docker compose down

logs:
  docker compose logs -f --tail=100

deploy-home host user app_dir="" ssh_port="22":
  ./scripts/deploy-home.sh "{{host}}" "{{user}}" "{{app_dir}}" "{{ssh_port}}"
