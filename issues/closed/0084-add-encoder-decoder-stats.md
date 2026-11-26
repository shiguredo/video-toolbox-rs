# エンコーダー / デコーダーの統計値 API を追加する

- Created: 2026-09-30
- Completed: 2026-10-02
- Branch: feature/add-encoder-decoder-stats
- Polished: {YYYY-MM-DD}

## 目的

video-toolbox-rs には内部状態を観測する手段が無く、利用側が「今どれだけフレームが滞っているか」「セッションが何回作り直されたか」を知る方法が無い。もう一方のバックエンドである nvcodec-rs は `Counter`（通算値）/ `Gauge`（時点値）と `EncoderStats` / `DecoderStats` を公開しており、Hisui 等の利用側がバックエンド非依存でメトリクスを扱える。本クレートにも同等の統計値 API を追加し、4 バックエンドで語彙を揃える。

特にデコーダーは `Decoder::update_format` が内部で `VTDecompressionSessionCanAcceptFormatDescription` の結果に応じてセッションを再作成するため、再作成が起きた事実が利用側から一切見えない。再作成はハンドラの差し替えと未出力フレームの扱いを伴うため、発生回数を計測できないと障害調査ができない。

## 現状

- `src/lib.rs` の公開 API に統計値関連の型が無く、`Encoder` / `Decoder` に `stats()` に相当するメソッドも無い
- `src/encoder.rs` の `Encoder` は `session` / `config` / `next_input_pts` / `handler` のみを保持しており、カウンターを保持していない
- `src/encoder/callback.rs` の `process_encoded_output` はエラー時に `EncodeHandler::on_encoded` へ `Err` を渡すだけで、エラー種別ごとの発生回数を記録していない
- `src/decoder.rs` の `Decoder` は `description` / `session` / `pixel_format` / `handler` のみを保持しており、`update_format` のセッション流用と再作成のどちらのパスを通ったかを記録していない
- エンコーダーは `VTCompressionSessionEncodeFrame` が受理したフレーム数の上限を公開していないため、利用側が「何フレーム送るごとに `Encoder::finish` を挟むべきか」を判断できない。nvcodec-rs は満杯を起こさない上限値を `EncoderStats::max_in_flight_frames`（`Gauge`）として公開し、README に flush 制御のレシピを載せている

## 設計方針

### 統計値の型

nvcodec-rs の `src/stats.rs` と同じ設計を本クレートにも置く。依存クレートは追加しない（nvcodec-rs も `AtomicU64` の薄いラッパーとして自前で定義している）。

- `Counter`: `AtomicU64` の薄いラッパーで単調増加の通算値。`new()` / `get()` を公開し、加算は crate 内専用。`Clone` は現在値の独立スナップショット
- `Gauge`: `AtomicU64` の薄いラッパーで時点値。`new()` / `get()` を公開し、設定は crate 内専用。`Clone` は現在値の独立スナップショット
- どちらも Relaxed order で読み書きする（happens-before を要求しない値のため）
- 公開は `src/lib.rs` からの再公開で行う。CODEBASE.md の「re-export の許可」に、統計値型の公開のために再公開を追加する理由を追記すること

### エンコーダー

- `EncoderStats` を新設し、少なくとも次を計測する
  - `total_encode_count` (`Counter`): `Encoder::encode` / `Encoder::encode_pixel_buffer` がフレーム送信まで到達した通算回数
  - `total_output_frame_count` (`Counter`): `EncodeHandler::on_encoded` に `Ok` を渡した通算回数
  - `total_error_count` (`Counter`): `EncodeHandler::on_encoded` に `Err` を渡した通算回数
  - `total_reconfigure_count` (`Counter`): `Encoder::reconfigure` が `VTSessionSetProperties` に成功した通算回数
  - `in_flight_frames` (`Gauge`): 送信済みでまだ出力コールバックが来ていないフレーム数の現在値
- `in_flight_frames` は `VTCompressionSessionEncodeFrame` 成功時に増やし、`process_encoded_output` でユーザーデータを回収した時に減らす。`VTCompressionSessionEncodeFrame` が失敗した場合は増やさない
- 数え方の精度（フレームドロップや `kVTEncodeInfo_FrameDropped` の扱い）は実装時に確定し、rustdoc に契約として書く
- `Encoder::stats(&self) -> &EncoderStats` を追加する。nvcodec-rs と同じく共有統計値への参照を返し、スナップショットが必要なら `clone()` させる
- nvcodec-rs の `max_in_flight_frames` に相当する「満杯を起こさない上限」を Video Toolbox から取得できるかは未確認のため、本 issue では扱わない。`in_flight_frames` の現在値が読めることで、利用側が上限を自分で決めて制御できる状態にする

### デコーダー

- `DecoderStats` を新設し、少なくとも次を計測する
  - `total_decode_count` (`Counter`): `Decoder::decode` がフレーム送信まで到達した通算回数
  - `total_output_frame_count` (`Counter`): `DecodeHandler::on_decoded` に `Ok` を渡した通算回数
  - `total_error_count` (`Counter`): `DecodeHandler::on_decoded` に `Err` を渡した通算回数
  - `total_create_session_count` (`Counter`): `VTDecompressionSessionCreate` に成功した通算回数（`Decoder::new` と `update_format` の再作成の合計）
  - `total_update_format_count` (`Counter`): `Decoder::update_format` がセッションを流用した通算回数
  - `total_recreate_session_count` (`Counter`): `Decoder::update_format` がセッションを再作成した通算回数
  - `in_flight_frames` (`Gauge`): 送信済みでまだ出力コールバックが来ていないフレーム数の現在値
- セッション流用と再作成のカウントは `Decoder::update_format` の `VTDecompressionSessionCanAcceptFormatDescription` の分岐で行う。`total_create_session_count` は再作成の回数と一致する（`Decoder::new` の 1 回を除く）
- `Decoder::stats(&self) -> &DecoderStats` を追加する

### スレッド安全性

コールバックは Video Toolbox のコールバック用スレッドから呼ばれるため、統計値は `Arc` で共有し、`AtomicU64` で読み書きする。`Counter` / `Gauge` は `Send + Sync` になるため `Encoder` / `Decoder` の `Send` 実装に影響しない。

## 完了条件

- `Counter` / `Gauge` が公開 API として利用できる
- `Encoder::stats()` / `Decoder::stats()` が統計値を返し、エンコード / デコードの進行に応じて値が増えることをテストで検証する
- `Decoder::update_format` のセッション流用パスと再作成パスで、対応するカウンターが増えることをテストで検証する
- `in_flight_frames` が送信で増え、出力コールバックで減ることをテストで検証する
- CODEBASE.md の「re-export の許可」に追記する
- README の設定 / 使い方に統計値の節を追加する
- CHANGES.md に `[ADD]` エントリを追加する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 解決方法

`src/stats.rs` に `Counter` (通算値) / `Gauge` (時点値) を新設し、`src/lib.rs` から再公開した。`Gauge` の crate 内 API は、`in_flight_frames` を送信側スレッドと出力コールバック側スレッドの双方から増減させるため、nvcodec-rs の `set` ではなく `inc` / `dec` とした。`dec` は 0 で飽和させ、増減の対応が崩れた場合に `u64` の桁溢れで巨大な値に見えるのを避けている。

`src/encoder/stats.rs` の `EncoderStats` と `src/decoder.rs` の `DecoderStats` を新設し、`Encoder::stats()` / `Decoder::stats()` が共有統計値への参照を返すようにした。`Encoder` / `Decoder` は統計値を `Arc` で保持し、Video Toolbox へ渡す refcon の `EncodeCallbackContext` / `DecodeCallbackContext` から同じ統計値を更新する。

`in_flight_frames` は送信の前に増やし、出力コールバックがユーザーデータを回収した時点 (ユーザーハンドラーの実行前) に減らす。`VTCompressionSessionEncodeFrame` / `VTDecompressionSessionDecodeFrame` はこの呼び出しから戻る前にコールバックを呼ぶことがあるため、送信後に増やすとコールバック側の減算が 0 で飽和して失われる。送信関数が失敗した場合はコールバックが来ないため、送信前に増やした分をその場で戻す。通算値の計上はユーザーハンドラーの実行前に行い、ハンドラーが panic しても計上済みの値が変わらないようにした。

`DecoderStats::total_update_format_count` / `total_recreate_session_count` は `Decoder::update_format` の `VTDecompressionSessionCanAcceptFormatDescription` の分岐で計上し、`total_create_session_count` は `VTDecompressionSessionCreate` の成功時に計上する。

テストは `src/stats.rs` と `src/encoder.rs` の単体テスト、`tests/test_encoder.rs` / `tests/test_decoder.rs` に追加した。`in_flight_frames` は、エンコーダーでは出力コールバックをハンドラー内でブロックして保留中の件数を直接検証し、デコーダーでは非同期デコードの完了順序に依存しない不変条件 (送信数 = in-flight + 出力 + エラー) で検証している。

README に「統計値」節、CODEBASE.md の「re-export の許可」に統計値型を再公開する理由、CHANGES.md に `[ADD]` エントリを追加した。
