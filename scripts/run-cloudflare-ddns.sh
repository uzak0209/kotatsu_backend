#!/usr/bin/env bash
set -euo pipefail

cd /Users/uzak/Projects/kotatsu/backend
set -a
source ./.env.ddns
set +a

./scripts/cloudflare-ddns-update.sh
