# デコード済みフレームのプレーン参照で `from_raw_parts` と算術が安全でない可能性がある

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`I420Frame` / `Nv12Frame` の `y_plane` 等で、`CVPixelBufferGetBaseAddressOfPlane` の戻りを `std::slice::from_raw_parts` に渡している。**プラットフォームの異常時に NULL が返る**可能性を完全に否定するなら、`NULL` を `from_raw_parts` に渡すのは **未定義動作**。また、スライス長を `height * stride` 等で算術しており、**乗算オーバーフロー**で短い長さと誤認すると、**実バッファより短いスライス**と見なし、後続の利用で **バッファ外**に繋がり得る。

## 現状

- **場所**: `src/lib.rs` の `I420Frame` / `Nv12Frame` の各 `*_plane` メソッドおよび `width` / `height` / `*_stride`
- `CVPixelBufferLockBaseAddress` は `decode` 内で成功している前提。

## 問題

- `GetBaseAddressOfPlane` が NULL の場合の分岐がない。
- `height * stride` 等の乗算オーバーフロー時の扱いがない。

## 望ましい対応の方向（案）

- NULL のときは空スライスやエラーにする方針を決める（公開 API の戻り型の変更が必要になる場合は設計判断）。
- `checked_mul` でスライス長を計算し、失敗時はパニックではなくエラーに寄せるか、ドキュメントで前提を限定する。

## 解決方法

`I420Frame` / `Nv12Frame` にプライベートの `plane_slice` を追加し、`row_count * bytes_per_row` の `checked_mul` を通した長さのみ `from_raw_parts` に渡す。ポインタが NULL のときは空スライスを返す。

## 解決方法（再対応）

`I420Frame` / `Nv12Frame` の公開ドキュメントと `plane_slice` のコメントに、異常時に空スライスになり得ることと、通常の正の解像度では空にならない想定である旨を記載した。
