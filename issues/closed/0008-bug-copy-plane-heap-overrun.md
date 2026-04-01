# copy_plane() で余剰データ付きスライスがヒープオーバーランを起こす

Created: 2026-03-31
Completed: 2026-03-31
Model: Composer 2 Fast

## 概要

`copy_plane()` の一括コピー分岐で `src.len()` バイトをそのまま `copy_nonoverlapping()` に渡している。`validate_frame_data()` は各プレーン長を「必要サイズ以上」でしか検証しないため、余剰データを含むスライスを渡すと CVPixelBuffer のプレーンバッファを超えて書き込む。

## 該当箇所

- `src/lib.rs:580` — `copy_nonoverlapping(src.as_ptr(), dst, src.len())`

## 解決方法

コピーサイズを `src.len()` から `src_width * src_height` に変更した。これにより CVPixelBuffer のプレーンバッファサイズを超える書き込みが発生しなくなる。
