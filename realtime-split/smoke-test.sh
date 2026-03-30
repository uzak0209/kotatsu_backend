#!/usr/bin/env bash
set -euo pipefail

cd /Users/uzak/Projects/kotatsu/backend/realtime-split
set -a
source ./.env.selfhost
set +a

BASE_URL="http://127.0.0.1:${API_PORT}"

echo "[1/4] health"
curl -fsSL "${BASE_URL}/health" | jq

echo "[2/4] create match"
CREATE_RES="$(curl -fsSL -X POST "${BASE_URL}/v1/matches" -H 'content-type: application/json' -d '{}')"
echo "$CREATE_RES" | jq
MATCH_ID="$(printf '%s' "$CREATE_RES" | jq -r '.match_id')"

echo "[3/4] join match"
curl -fsSL -X POST "${BASE_URL}/v1/matches/${MATCH_ID}/join" -H 'content-type: application/json' -d '{"display_name":"smoke"}' | jq

echo "[4/4] delete match"
DELETE_STATUS="$(
  curl -sS -o /dev/null -w '%{http_code}' -X DELETE "${BASE_URL}/v1/matches/${MATCH_ID}"
)"
test "$DELETE_STATUS" = "204"
echo "$DELETE_STATUS"
