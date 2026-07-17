# `is_keyframe` が `CFArray` の要素数を確認せず `CFArrayGetValueAtIndex(attachments, 0)` を呼ぶ

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`is_keyframe` は `CMSampleBufferGetSampleAttachmentsArray` が非 NULL のあと、すぐ `CFArrayGetValueAtIndex(attachments, 0)` を呼んでいる。添付配列が **要素 0 件**のとき、インデックス 0 は範囲外であり、CoreFoundation の契約上 **未定義**。クラッシュや不正なポインタ取得のリスクがある。

## 現状

- **場所**: `src/lib.rs` の `is_keyframe`
- `attachments.is_null()` のみチェックし、`CFArrayGetCount(attachments)` は呼んでいない。

## 問題

- 要素数が 0 のとき、`CFArrayGetValueAtIndex(attachments, 0)` の挙動は保証されない。

## 望ましい対応の方向（案）

- `CFArrayGetCount(attachments) == 0` のときは `false` を返す（またはキーフレームでない扱い）など、先に件数を確認する。
- 可能なら境界に近いテスト（モックが難しければコードレビューとドキュメントで担保）。

## 解決方法

`is_keyframe` 内で `CFArrayGetCount(attachments) == 0` のときは `false` を返すようにした。

## 解決方法（再対応）

`CFGetTypeID` と `CFDictionaryGetTypeID` を比較し、添付が辞書でない場合は `false` を返す。
