# `Decoder::decode` が `&[u8]` からミュータブルポインタを `CMBlockBufferCreateWithMemoryBlock` に渡している

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`CMBlockBufferCreateWithMemoryBlock` に `data.as_ptr().cast_mut()` を渡している。Rust のエイリアス規則上、**`&[u8]` が指すメモリを C が書き換える**なら **未定義動作**になり得る。API が **読み取り専用**であれば実務上は問題になりにくいが、**書き込みの有無がコード上明示されていない**ため、厳密な意味ではリスクが残る。

## 現状

- **場所**: `src/lib.rs` の `Decoder::decode`
- `data.as_ptr().cast_mut().cast()` をブロックバッファのメモリブロックとして渡している。

## 問題

- `CMBlockBufferCreateWithMemoryBlock` が当該メモリに書き込むかどうかが、Rust 側から保証されていない。

## 望ましい対応の方向（案）

- Apple のドキュメントで「読み取り専用か」を確認し、**読み取りのみ**であれば `unsafe` ブロック内に根拠コメントを書く。
- 書き込みがあり得るなら、`Vec` へのコピーや `UnsafeCell` 等、別の所有権モデルを検討する。

## 解決方法

`Decoder::decode` で入力 `&[u8]` を `to_vec()` し、`Vec` が所有するバッファのミュータブルポインタを `CMBlockBufferCreateWithMemoryBlock` に渡すようにした。日本語コメントで、エイリアス規則上の理由を記載した。

## 解決方法（再対応）

`decode` のドキュメントコメントに、`owned` の生存期間、`VTDecompressionSessionDecodeFrame` の同期完了、`kCFAllocatorNull` による解放されないヒープ領域である旨を追記した。
