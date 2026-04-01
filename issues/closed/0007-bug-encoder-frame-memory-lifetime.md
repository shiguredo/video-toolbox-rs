# encode() で入力フレームのメモリ寿命が非同期エンコード中に保証されない

Created: 2026-03-31
Completed: 2026-03-31
Model: Composer 2 Fast

## 概要

`CVPixelBufferCreateWithPlanarBytes` に `y/u/v/uv` の生ポインタをそのまま渡しており、解放コールバック (`releaseCallback`) を設定していない。ピクセルバッファが参照する実メモリの所有者は呼び出し元の `&[u8]` のままである。

`VTCompressionSessionEncodeFrame()` が CVPixelBuffer を retain するのは CVPixelBuffer オブジェクトであって、外部プレーンメモリの所有権ではない。`encode()` が返った後に呼び出し側が入力バッファを再利用した場合、Video Toolbox がまだ外部プレーンを読んでいると破損またはクラッシュする。

## 該当箇所

- `src/lib.rs:625` — `CVPixelBufferCreateWithPlanarBytes` (I420)
- `src/lib.rs:647` — `CVPixelBufferCreateWithPlanarBytes` (NV12)
- `src/lib.rs:687` — `VTCompressionSessionEncodeFrame` の呼び出し

## 修正方針

`CVPixelBufferCreateWithPlanarBytes` を `CVPixelBufferCreate` + `CVPixelBufferLockBaseAddress` + データコピー + `CVPixelBufferUnlockBaseAddress` に置き換える。CoreVideo がメモリを所有するため、入力バッファの寿命に依存しなくなる。

## 解決方法

`CVPixelBufferCreate` で CoreVideo にメモリを確保させ、`CVPixelBufferLockBaseAddress` でロックした上で `copy_plane` ヘルパーで入力データをコピーし、`CVPixelBufferUnlockBaseAddress` でアンロックする方式に変更した。ストライドが入力幅と異なる場合は行ごとにコピーする。
