# Cloudflare DDNS Setup (PC script)

## 1. Prepare Cloudflare values
- `CF_ZONE_ID`
- `CF_API_TOKEN` (Zone.DNS Edit permission for target zone)
- `CF_RECORD_NAME` (e.g. `home.example.com`)

## 2. Run once (dry-run)
```bash
set -a; source /Users/uzak/Projects/kotatsu/backend/.env.ddns; set +a
DRY_RUN=true \
/Users/uzak/Projects/kotatsu/backend/scripts/cloudflare-ddns-update.sh
```

## 3. Run actual update
```bash
set -a; source /Users/uzak/Projects/kotatsu/backend/.env.ddns; set +a
/Users/uzak/Projects/kotatsu/backend/scripts/cloudflare-ddns-update.sh
```

## 4. Automate (every 5 minutes)
```bash
crontab -e
```
Add:
```cron
*/5 * * * * CF_API_TOKEN=xxx CF_ZONE_ID=yyy CF_RECORD_NAME=home.example.com /Users/uzak/Projects/kotatsu/backend/scripts/cloudflare-ddns-update.sh >> $HOME/cloudflare-ddns.log 2>&1
```

## Optional vars
- `CF_RECORD_TYPE`: `A` or `AAAA` (default `A`)
- `IP_CHECK_URL`: default `https://api.ipify.org`
- `STATE_FILE`: local cache file path
