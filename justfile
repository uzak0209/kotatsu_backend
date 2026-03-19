set shell := ["zsh", "-cu"]

default:
  @just --list

check:
  cargo check --manifest-path realtime-split/Cargo.toml

test:
  cargo test --manifest-path realtime-split/Cargo.toml

up:
  docker compose up --build -d

down:
  docker compose down

logs:
  docker compose logs -f --tail=100

deploy-home host user app_dir="/opt/kotatsu-backend" ssh_port="22":
  ./scripts/deploy-home.sh "{{host}}" "{{user}}" "{{app_dir}}" "{{ssh_port}}"
