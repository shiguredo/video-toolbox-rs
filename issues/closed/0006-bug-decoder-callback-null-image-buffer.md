# デコーダーの output_callback で image_buffer の NULL チェックが不足している

Created: 2026-03-31
Completed: 2026-03-31
Model: Opus 4.6

## 概要

`Decoder::output_callback()` で `status` が OK の場合でも、`info_flags` に `kVTDecodeInfo_FrameDropped` が立っていると `image_buffer` が NULL になりうる。現状は NULL チェックなしで `CFRetain(image_buffer)` を呼んでおり、セグフォの原因になる。

## 該当箇所

- `src/lib.rs:1388` — `output_callback` 関数
- `src/lib.rs:1404` — `CFRetain(image_buffer.cast())` の呼び出し

## 補足

`decode()` 側（1363 行目）では `image_buffer.is_null()` チェックが入っているが、コールバック内で NULL ポインタを `CFRetain` してしまうと、その後の `decode()` 側のチェックに到達する前にクラッシュする。

現状 `decode_flags = 0`（同期デコード）なのでフレームドロップは起きにくいが、防御的に NULL チェックを追加すべき。

## 修正方針

`output_callback` 内で `image_buffer.is_null()` の場合は `CFRetain` をスキップし、early return する。

## 解決方法

デコーダーの `output_callback` とエンコーダーの `process_encoded_output` の両方に NULL チェックを追加した。NULL の場合は早期リターンする。
