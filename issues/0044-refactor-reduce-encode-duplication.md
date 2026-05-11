# `encode` と `encode_pixel_buffer` の重複ロジックを共通化する

Created: 2026-05-11
Model: deepseek-v4-pro

## 概要

`Encoder::encode` と `Encoder::encode_pixel_buffer` で以下の処理が約 37 行にわたって重複している:

- `frame_properties` 辞書の生成（`force_key_frame` 判定）
- `source_frame_ref_con` の `Box::into_raw` による作成とエラー時の `from_raw` によるリーク防止
- `VTCompressionSessionEncodeFrame` の呼び出しとエラーハンドリング
- `next_input_pts` の `checked_add` とオーバーフローエラー処理

## 背景

`encode_pixel_buffer` はゼロコピー用の unsafe API であり、`encode` は通常のデータコピー用の safe API である。
両者で CVPixelBuffer の準備方法は異なるが、`VTCompressionSessionEncodeFrame` 呼び出し以降の処理は完全に同一である。

## 対応方針

共通部分を private なヘルパー関数に抽出する:

```rust
unsafe fn encode_frame_impl(
    &mut self,
    image_buffer: &CfPtrMut<sys::__CVBuffer>,
    options: &EncodeOptions,
    user_data: T,
) -> Result<(), Error> {
    // frame_properties 生成
    // source_frame_ref_con 作成
    // VTCompressionSessionEncodeFrame 呼び出し
    // エラーハンドリング
    // PTS 更新
}
```

## 変更対象ファイル

- `src/encoder.rs`: 共通ヘルパー抽出、`encode` と `encode_pixel_buffer` を呼び出し元に変更

## 注意点

- `encode` 側は `CVPixelBufferCreate` + `copy_plane`、`encode_pixel_buffer` 側は `CFRetain` 済みポインタを渡す。ヘルパー関数の引数は `CfPtrMut` を受け取る形で統一する。
- 回帰テストとして既存の `test_encoder.rs` の全テストが通過することを確認する。
