# `process_encoded_output` で `CMBlockBufferGetDataPointer` の戻り長を全データ長として扱うと非連続バッファでフレームが切り詰められる

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`CMBlockBufferGetDataPointer` の `lengthAtOffsetOut`（本コードでは `data_pointer_len`）は、**指定オフセットから連続して見えている領域の長さ**であり、**ブロックバッファ全体のデータ長ではない**。`CMBlockBuffer` が非連続な場合、`CMBlockBufferGetDataLength` で得られる **`block_len` より `data_pointer_len` が小さく**なり得る。その状態で **`data_pointer_len` だけを `Vec` にコピーすると、圧縮ビットストリームの途中までしか渡らず破損フレーム**になる。

`data_pointer_len > block_len` だけを弾いても、**短い方をそのまま採用する誤り**は防げない。

## 現状

- **場所**: `src/lib.rs` の `Encoder::process_encoded_output`

## 問題

- 連続領域の長さをブロック全体長とみなしてコピーしている。

## 解決方法

`CMBlockBufferGetDataLength` で **`block_len`** を取得し、**`CMBlockBufferCopyDataBytes`** でオフセット 0 から **`block_len` バイト**を宛先 `Vec` にコピーするようにした。非連続バッファでも **全長**が取れる。
