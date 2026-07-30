# PTS オーバーフロー検査がフレーム送信後に行われる

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-pts-overflow-check-order
- Polished: {YYYY-MM-DD}

## 目的

`Encoder::encode` と `Encoder::encode_pixel_buffer` で、`next_input_pts` のオーバーフロー検査（`checked_add`）が `VTCompressionSessionEncodeFrame` 呼び出し**後**に行われている。オーバーフロー時に `Err` を返すが、フレームは既に非同期で送信済みであり、呼び出し側は失敗と認識するがフレームは in-flight になる。

## 現状

`src/encoder.rs` の `encode` 関数と `encode_pixel_buffer` 関数で、`VTCompressionSessionEncodeFrame` の成功後に `self.next_input_pts.checked_add(self.config.fps_denominator as i64)` を実行している。

## 設計方針

`checked_add` を `VTCompressionSessionEncodeFrame` 呼び出し**前**に実行し、オーバーフローするなら送信せずにエラーを返す。

## 完了条件

PTS がオーバーフローする場合、フレームを送信せずに `Error::LimitExceeded` を返すこと。
