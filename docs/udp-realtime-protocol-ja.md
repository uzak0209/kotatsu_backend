# Kotatsu UDP リアルタイム通信仕様

この文書は、`realtime-split/realtime-server` が使っている UDP 通信の実装仕様を、日本語で整理したものです。

HTTP のマッチメイク API については、別途 [matchmaking-api.md](/Users/uzak/Projects/kotatsu/backend/docs/matchmaking-api.md) を参照してください。

## 全体の流れ

1. まず HTTP API でマッチを作る
2. HTTP API で各プレイヤーが join し、`token` と `udp_url` を受け取る
3. 必要ならホストが HTTP `POST /v1/matches/{match_id}/start` を呼ぶ
4. クライアントは `udp_url` のホストとポートへ UDP socket を接続する
5. 最初に Reliable packet として `join` メッセージを送る
6. サーバから `join_ok` が返ってきたら参加完了
7. 以降は:
   - Reliable packet でパラメータ変更
   - Datagram packet で位置同期

## 接続前に必要なもの

HTTP の `POST /v1/matches/{match_id}/join` が返す以下の値を使います。

- `udp_url`
  - 例: `udp://kotatsu.ruxel.net:4433`
- `token`
  - 最初の `join` 認証に使うトークン
- `player_id`
  - HTTP 側で払い出されるプレイヤー ID

## パケット構造

すべての UDP payload は、先頭 1 byte の packet type と、その後ろの JSON bytes で構成されます。

- `0x01`: Reliable packet
- `0x02`: Datagram packet

つまり wire format は次の形です。

```text
[packet_type:1byte][json payload...]
```

## Reliable packet (`0x01`)

Reliable packet は、順序が重要な制御メッセージに使います。

### クライアントから送るもの

#### `join`

最初の Reliable packet は必ず `join` です。

```json
{
  "t": "join",
  "token": "01234567-89ab-cdef-0123-456789abcdef"
}
```

#### `param_change`

```json
{
  "t": "param_change",
  "seq": 1,
  "param": "gravity",
  "direction": "increase"
}
```

フィールド:
- `seq`: クライアント側の連番
- `param`: `gravity` / `friction` / `speed`
- `direction`: `increase` / `decrease`

現在の制約:
- `gravity`: 1 から 3
- `speed`: 1 から 3
- `friction`: 1 から 2
- 初期値は全て `2`
- 変更後は 30 秒クールダウン

### サーバから返るもの

#### `join_ok`

```json
{
  "t": "join_ok",
  "match_id": "m_0123456789abcdef0123456789abcdef",
  "player_id": "p_0123456789abcdef0123456789abcdef",
  "params": {
    "gravity": 2,
    "friction": 2,
    "speed": 2
  },
  "server_time_ms": 1761000000123
}
```

#### `match_started`

```json
{
  "t": "match_started",
  "match_id": "m_0123456789abcdef0123456789abcdef",
  "started_at_unix": 1761000000,
  "server_time_ms": 1761000000450
}
```

#### `param_applied`

```json
{
  "t": "param_applied",
  "from_player_id": "p_0123456789abcdef0123456789abcdef",
  "seq": 1,
  "params": {
    "gravity": 3,
    "friction": 2,
    "speed": 2
  },
  "next_param_change_at_unix": 1761000030,
  "server_time_ms": 1761000000456
}
```

#### `error`

```json
{
  "t": "error",
  "code": "param_update_failed",
  "message": "cooldown_active:1761000030"
}
```

主な `code`:
- `invalid_first_message`
- `auth_failed`
- `join_failed`
- `param_update_failed`

## Datagram packet (`0x02`)

Datagram packet は、落ちてもよい高頻度の位置同期用です。

### クライアントから送る `pos`

```json
{
  "t": "pos",
  "seq": 42,
  "x": 12.3,
  "y": 4.5,
  "vx": 0.1,
  "vy": -0.2
}
```

### サーバから返る `pos`

サーバは受け取った `pos` を、送信者以外の同じマッチのプレイヤーへ broadcast します。

```json
{
  "t": "pos",
  "player_id": "p_other_player",
  "seq": 42,
  "x": 12.3,
  "y": 4.5,
  "vx": 0.1,
  "vy": -0.2,
  "server_time_ms": 1761000000789
}
```

## 実装上の注意

- Reliable と Datagram は同じ UDP socket を共有します
- Reliable packet も Datagram packet も JSON bytes をそのまま載せます
- `join` より前の Reliable packet は受け付けません
- 不正な Datagram packet はサーバ側で黙って破棄されます
- `token` は消費型なので再利用すると失敗します

## 最小の接続シーケンス

1. HTTP `POST /v1/matches`
2. HTTP `POST /v1/matches/{match_id}/join`
3. 必要ならホストが HTTP `POST /v1/matches/{match_id}/start`
4. `udp_url` に UDP socket を接続
5. Reliable packet (`0x01`) で `{"t":"join","token":"..."}` を送る
6. `join_ok` を受け取る
7. 開始後は `match_started` を受け取る
8. 必要に応じて:
   - Reliable packet で `param_change`
   - Datagram packet (`0x02`) で `pos`

## 参考実装

- サーバ実装: `realtime-split/realtime-server/src/main.rs`
- UDP handler: `realtime-split/realtime-server/src/udp_connection.rs`
- テストクライアント: `realtime-split/test-client/src/main.rs`
