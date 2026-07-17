# `copy_plane` の `src_width * src_height` が `usize` でオーバーフローし得る

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`copy_plane` で `copy_size = src_width * src_height` を計算し、`copy_nonoverlapping` の長さに使っている。`usize` の乗算が **オーバーフロー**すると Rust では **ラップ**され、**意図より小さな `copy_size`** になる。結果として **ピクセルバッファの一部が未初期化のままエンコードに渡る**など、論理的な不整合や未規定データのリスクが理論上ある。異常に大きな `width` / `height` を許すと顕在化しやすい。

## 現状

- **場所**: `src/lib.rs` の `Encoder::copy_plane`
- `width` / `height` は `encode` から `self.config.width` / `height` 由来の `usize` に基づく。

## 問題

- `checked_mul` 等がなく、乗算オーバーフロー時の挙動が暗黙のラップになる。

## 望ましい対応の方向（案）

- `checked_mul` で失敗時はエラーにする、または `EncoderConfig` の幅・高さの上限を検証する。
- 一括コピーと行ループの両方で一貫した検証を行う。

## 解決方法

`copy_plane` を `Result<(), Error>` にし、`src_width * src_height` および行方向のオフセットに `checked_mul` を用いる。失敗時は `Error::LimitExceeded` を返す。呼び出し元の `encode` で `?` により伝播する。

## 解決方法（再対応）

`copy_plane` で `CVPixelBufferGetBaseAddressOfPlane` が NULL の場合は `LimitExceeded` を返す。`validate_frame_data` は `frame_byte_len_checked` により `width * height` 等の乗算を `checked_mul` で行う。

## 解決方法（レビュー追補）

追加レビューで、`dst_stride != src_width` の行コピー経路において **`dst_stride < src_width`** だと **1 行分の宛先バッファを超えて `copy_nonoverlapping` する**可能性があったため修正した。

- `CVPixelBufferGetBytesPerRowOfPlane` の戻りが `src_width` 未満の場合は `Error::LimitExceeded`（`plane destination stride is less than copy width`）を返す。
