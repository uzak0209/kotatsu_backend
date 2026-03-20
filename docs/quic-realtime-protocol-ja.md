# Kotatsu QUIC リアルタイム通信仕様

この文書は、`realtime-split/realtime-server` が使っている QUIC 通信の実装仕様を、日本語でわかりやすく整理したものです。

対象は次の 2 つです。
- Reliable QUIC stream
- Unreliable QUIC datagram

HTTP のマッチメイク API については、別途 [matchmaking-api.md](/Users/uzak/Projects/kotatsu/backend/docs/matchmaking-api.md) を参照してください。

## 全体の流れ

1. まず HTTP API でマッチを作る
2. HTTP API で各プレイヤーが join し、`token` と `quic_url` を受け取る
3. クライアントは `quic_url` に QUIC 接続する
4. 接続後、最初に Reliable stream で `join` メッセージを送る
5. サーバから `join_ok` が返ってきたら参加完了
6. 以降は:
   - Reliable stream でパラメータ変更
   - Datagram で位置同期

## 接続前に必要なもの

HTTP の `POST /v1/matches/{match_id}/join` が返す以下の値を使います。

- `quic_url`
  - 例: `quic://kotatsu.ruxel.net:4433`
- `token`
  - QUIC 接続後の join 認証に使うトークン
- `player_id`
  - HTTP 側で払い出されるプレイヤー ID

## QUIC 接続方法

### 1. `quic_url` に接続する

`quic_url` のホストとポートに QUIC で接続します。

例:
- `quic://kotatsu.ruxel.net:4433`

実装上は `quic://` を `https://` として URL パースし、ホスト名とポート番号を取り出して接続しています。

### 2. Bidirectional stream を 1 本開く

接続直後に、クライアントは QUIC の bidirectional stream を 1 本開きます。

この stream は Reliable channel として使われます。

### 3. 最初のメッセージとして `join` を送る

最初の Reliable メッセージは必ず `join` でなければいけません。

送信例:
```json
{"t":"join","token":"01234567-89ab-cdef-0123-456789abcdef"}
```

Reliable stream の JSON は 1 メッセージごとに改行 `\n` を付けて送ります。

つまり実際には次のようなイメージです。
```text
{"t":"join","token":"..."}\n
```

## Reliable stream で送るもの

Reliable stream は、順序保証あり・欠落なしで扱いたいメッセージに使います。

今の実装では次の 2 種類です。

### 1. `join`

接続直後に 1 回だけ送るメッセージです。

形式:
```json
{
  "t": "join",
  "token": "join API で受け取った token"
}
```

補足:
- 最初のメッセージが `join` 以外だとエラーになります
- `token` は実質 1 回限りです
- 期限切れや再利用時は `auth_failed` 系のエラーになります

### 2. `param_change`

プレイヤーのパラメータ変更を Reliable stream で送ります。

形式:
```json
{
  "t": "param_change",
  "seq": 1,
  "param": "gravity",
  "direction": "increase"
}
```

フィールド:
- `seq`
  - クライアント側の連番
  - 現状は主にそのままサーバから返される識別子として使われます
- `param`
  - `gravity`
  - `friction`
  - `speed`
- `direction`
  - `increase`
  - `decrease`

現在のパラメータ制約:
- `gravity`: 1 から 3
- `speed`: 1 から 3
- `friction`: 1 から 2
- 初期値は全て `2`
- 変更後は 30 秒クールダウン

## Reliable stream で返ってくるもの

### 1. `join_ok`

`join` 成功後、最初に返る正常メッセージです。

例:
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

意味:
- `params`
  - join 直後の自分の現在パラメータ
- `server_time_ms`
  - サーバ時刻の Unix milliseconds

### 2. `param_applied`

誰かのパラメータ変更が反映されると、同じマッチ内の全プレイヤーへ broadcast されます。

例:
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

意味:
- `from_player_id`
  - 誰の変更か
- `seq`
  - クライアントが送った `param_change.seq`
- `params`
  - 変更後のそのプレイヤーの状態
- `next_param_change_at_unix`
  - 次に変更可能になる Unix seconds

### 3. `error`

Reliable stream 上のエラー通知です。

例:
```json
{
  "t": "error",
  "code": "param_update_failed",
  "message": "cooldown_active:1761000030"
}
```

主な `code`:
- `invalid_first_message`
  - 最初のメッセージが `join` ではなかった
- `auth_failed`
  - `token` が不正、期限切れ、または再利用された
- `join_failed`
  - join 自体に失敗した
- `param_update_failed`
  - パラメータ変更に失敗した

主な `message`:
- `invalid_token`
- `token_expired`
- `match_full`
- `match_not_found`
- `cooldown_active:<unix_seconds>`
- `out_of_range`

## Datagram で送るもの

Datagram は、落ちてもよい高頻度の位置同期用です。

今の実装では `pos` だけを送ります。

例:
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

フィールド:
- `seq`
  - 位置更新の連番
- `x`, `y`
  - 座標
- `vx`, `vy`
  - 速度

補足:
- Datagram は改行不要です
- そのまま JSON bytes を 1 datagram として送ります
- 順序保証はありません
- 到達保証もありません

## Datagram で返ってくるもの

サーバは受け取った `pos` を、送信者以外の同じマッチのプレイヤーへ broadcast します。

例:
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

ポイント:
- 自分が送った datagram は自分には返ってきません
- `player_id` が付くので、誰の位置更新か分かります
- `server_time_ms` が付くので、クライアント側補間や計測に使えます

## 実装上の注意

### 1. Reliable と Datagram は役割が違う

- Reliable stream:
  - join 認証
  - パラメータ変更
  - エラー通知
- Datagram:
  - 位置同期

### 2. 最初の stream メッセージは必ず `join`

これを守らないと参加できません。

### 3. `token` は使い捨てに近い

サーバは token を消費してから認証するため、再利用は失敗します。

### 4. 不正な datagram は黙って捨てられる

Datagram の JSON パースに失敗した場合、サーバは特にエラーを返さず無視します。

### 5. 現在のテストクライアントは証明書検証を外している

現在の実装では、サーバは自己署名証明書を使っています。
そのため、手元のクライアント実装では証明書検証を無効化して接続しています。

本番クライアントでは、将来的に以下のどちらかが必要です。
- 正しいサーバ証明書を使う
- 自前の検証ロジックを入れる

## 最小の接続シーケンス

1. HTTP `POST /v1/matches`
2. HTTP `POST /v1/matches/{match_id}/join`
3. `quic_url` に QUIC 接続
4. bidirectional stream を開く
5. Reliable stream で `{"t":"join","token":"..."}` を送る
6. `join_ok` を受け取る
7. 必要に応じて:
   - Reliable stream で `param_change`
   - Datagram で `pos`

## 参考実装

- サーバ実装: `realtime-split/realtime-server/src/main.rs`
- テストクライアント: `realtime-split/test-client/src/main.rs`
