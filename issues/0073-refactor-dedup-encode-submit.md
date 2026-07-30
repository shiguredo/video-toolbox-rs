# `encode` / `encode_pixel_buffer` の重複を解消する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-dedup-encode-submit
- Polished: {YYYY-MM-DD}

## 目的

`src/encoder.rs` の `encode` と `encode_pixel_buffer` で、`frame_properties` 構築・`VTCompressionSessionEncodeFrame` 呼び出し・エラー時の Box 回収・`next_input_pts` 加算が約 40 行にわたり同一。バグ修正時に片方だけ漏れるリスクがある（0061 の PTS 検査順序修正がまさに両方に必要）。

## 現状

両メソッドで以下のブロックが完全に重複:

- `frame_properties` の `cf_dictionary` 構築とガード
- `source_frame_ref_con` の `Box::into_raw`
- `VTCompressionSessionEncodeFrame` 呼び出しとエラー時の Box 回収
- `next_input_pts` の `checked_add`

## 設計方針

「CVPixelBuffer を受け取ってエンコードに投入する」共通 private メソッド（例: `submit_pixel_buffer`）を抽出し、両メソッドはバッファの取得方法（コピー or ゼロコピー）だけ担当する形にする。

## 完了条件

`VTCompressionSessionEncodeFrame` 呼び出し周りのロジックが 1 箇所に集約されていること。公開 API の変更はないこと。
