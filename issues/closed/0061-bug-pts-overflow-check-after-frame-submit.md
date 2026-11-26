# PTS オーバーフロー検査がフレーム送信後に行われる

- Created: 2026-07-30
- Completed: 2026-08-06
- Branch: feature/fix-pts-overflow-check-order
- Polished: 2026-08-01

## 目的

`Encoder::encode` と `Encoder::encode_pixel_buffer` で、`next_input_pts` のオーバーフロー検査（`checked_add`）が `VTCompressionSessionEncodeFrame` 呼び出し**後**に行われている。オーバーフロー時に `Err` を返すが、フレームは既に非同期で送信済みであり、呼び出し側は失敗と認識するがフレームは in-flight になる。さらに `next_input_pts` が進まないため、次回呼び出しで同一 PTS が再送される。

## 現状

`src/encoder.rs` の `encode` 関数と `encode_pixel_buffer` 関数で、`VTCompressionSessionEncodeFrame` の成功後に `self.next_input_pts.checked_add(self.config.fps_denominator as i64)` を実行している。オーバーフロー時は `Error::LimitExceeded` を返す（`reason: "input presentation timestamp overflow"`）。

## 設計方針

`checked_add` を `VTCompressionSessionEncodeFrame` 呼び出し**前**に実行し、オーバーフローするなら送信せずにエラーを返す。`reconfigure` の `rescaled_next_input_pts` パターン（FFI 実行前に計算値を確定し、成功後に `self` へ反映）と同じ構成にする。

実装構造:

- 送信前に `new_value = self.next_input_pts.checked_add(self.config.fps_denominator as i64)` を検査し、`None` なら `Error::LimitExceeded` を返す（この時点では `next_input_pts` を変更しない）。検査は `Box::into_raw`（`user_data` の Box 化）より前に行い、オーバーフロー時は `user_data` が通常の drop で解放されるようにする
- `VTCompressionSessionEncodeFrame` には `CMTimeMake(self.next_input_pts, ...)` で現在の `self.next_input_pts` をそのまま渡す（`new_value` は送信 PTS には使わない。先頭フレームが PTS 0 から始まる現行の PTS 系列を変えない）
- `VTCompressionSessionEncodeFrame` 成功後にのみ `self.next_input_pts = new_value` を代入する（送信失敗時は現状どおり `next_input_pts` を進めない）
- エラー時の `reason` は既存の `"input presentation timestamp overflow"` を維持する

なお、`next_input_pts = i64::MAX` のとき、そのフレームの PTS（`i64::MAX`）自体は有効な CMTime だが、加算がオーバーフローするため修正後は送信されない（修正前は送信されていた）。またエラー後は `next_input_pts` が動かないため、以後の `encode` / `encode_pixel_buffer` はすべて同じ `Error::LimitExceeded` を返す（`encode` 系 API のみでは回復しない。`reconfigure` の再スケールでは回復し得る）。実質到達不能な境界であり、挙動の変更は意図どおり。

### 検証方法

オーバーフローは `next_input_pts` が `i64::MAX` 付近に到達した場合のみ発生し、公開 API 経由では誘発できない。既存の `reconfigure_overflows_when_rescaled_pts_exceeds_i64_max` と同じ手法で private フィールドへ直接 `i64::MAX` を書き込むモジュール内テストを作成し、以下を検証する:

- `encode` / `encode_pixel_buffer` が `Error::LimitExceeded` を返すこと
- `next_input_pts` が不変であること
- 出力コールバックが発火しないこと（フレームを送信しないことの検証。修正前後を区別できるのはこの項目のみ）。発火しないことの検証は「一定時間待ってカウント 0」で行うため、陽性対照（正常フレームを 1 枚送ってコールバックが届くことの確認）を併設し、環境がコールバックを一切出さない場合の空振りを防ぐ。陽性対照は別フェーズで行い、オーバーフロー検証のコールバックカウントの基準値は陽性対照の完了後に取る

`encode_pixel_buffer` のテストは本ライブラリで初となるため、`sys::CVPixelBufferCreate` による有効な CVPixelBuffer の生成が前準備として必要になる。

## 完了条件

- PTS がオーバーフローする場合、フレームを送信せずに `Error::LimitExceeded` を返すこと（出力コールバックが発火しないことの検証を含む）
- `CHANGES.md` の `## develop` に `[FIX]` としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0073: `encode` / `encode_pixel_buffer` の重複解消を対象とし、同一のコード領域を変更する。0073 側から本 issue の修正（両関数への適用）を前提としているため、本 issue を先に実施し、その後 0073 を実施する
- issue 0053: `encode` 系の `pixel_buffer.rs` への移動が同一領域に触れる
- issue 0055: `cf_dictionary` の所有権統一が同一領域に触れる

## 解決方法

- `src/encoder/pixel_buffer.rs` の `Encoder::encode` / `Encoder::encode_pixel_buffer` で、
  `next_input_pts` の `checked_add` を `VTCompressionSessionEncodeFrame` 呼び出し前に移動した
  - 入力検証（ピクセルフォーマット / データ長）を先に行い、その後に PTS 検査を実行する。
    オーバーフロー時はフレームを送信せずに `Error::LimitExceeded`
    （`reason: "input presentation timestamp overflow"`）を返し、`next_input_pts` を変更しない
  - 送信 PTS は従来どおり現在の `self.next_input_pts` を渡し、`VTCompressionSessionEncodeFrame`
    成功後にのみ `self.next_input_pts = new_next_input_pts` を代入する（送信失敗時は進めない）
  - 検査が `Box::into_raw` より前にあるため、オーバーフロー時に `user_data` は通常の drop で
    解放される（旧実装では送信後に検査するため、Box 化済みの `user_data` がリークし得た）
- テスト（`src/encoder.rs` の `#[cfg(test)]` モジュール）
  - `encode_rejects_pts_overflow_before_frame_submit` / `encode_pixel_buffer_rejects_pts_overflow_before_frame_submit`
    を追加した。`next_input_pts` へ直接 `i64::MAX` を書き込み、オーバーフロー時に
    送信 API がエラーを返すこと・`next_input_pts` が不変であること・以後の呼び出しも
    同じエラーを返すこと・出力コールバックが発火しないことを検証する
  - コールバック非発火の検証は「一定時間待ってカウント 0」のため、陽性対照
    （正常フレーム 1 枚を送ってコールバックが届くこと）を併設し、基準値は陽性対照の
    完了後に取得する
  - `encode_pixel_buffer` のテストは `sys::CVPixelBufferCreate` で有効な CVPixelBuffer を
    生成して実行する
- 完了条件の確認結果
  - オーバーフロー時にフレームが送信されないこと（コールバック非発火）をテストで確認
  - `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` /
    `cargo fmt --all -- --check` がすべて通ることを確認
