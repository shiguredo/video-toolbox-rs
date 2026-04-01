# `process_encoded_output` で `CMSampleBufferGetDataBuffer` の戻りが NULL のときに `CMBlockBufferGetDataPointer` を呼び得る

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

エンコード出力コールバック `process_encoded_output` は、圧縮ビットストリームを `CMBlockBuffer` 経由で読み取る。ここで **`CMSampleBufferGetDataBuffer`** が返す `CMBlockBufferRef` が **NULL** である可能性をコード上無視している。NULL のブロックバッファを **`CMBlockBufferGetDataPointer`** に渡したときの挙動は、Apple の API としても **呼び出し側が不正な引数を渡さない**前提に近く、**NULL デリファレンスによるセグメンテーション違反**や、実装依存の未定義動作に繋がり得る。

本庫は **性能より堅牢性を優先する**方針であるため、ここは **明示的な NULL チェックと早期 return**（または同等のエラー処理）に寄せるのが望ましい。

## 背景（API の流れ）

1. `CMSampleBufferRef` 自体は `sample_buffer.is_null()` で弾いている。
2. `let data_buffer = CMSampleBufferGetDataBuffer(sample_buffer)` でデータ用 `CMBlockBufferRef` を取得する。
3. `CMBlockBufferGetDataPointer(data_buffer, ...)` でポインタと長さを得る。

手順 2 で **データバッファが無いサンプル**（フォーマットやエラー経路によっては NULL が返り得る）では、手順 3 に **NULL を渡す**ことになる。

## 現状

- **ファイル**: `src/lib.rs`
- **関数**: `Encoder::process_encoded_output`（`unsafe fn` 内のロジック）
- **処理**: `sample_buffer` の NULL チェックの直後に `CMSampleBufferGetDataBuffer` を呼び、その戻り値 **`data_buffer` を NULL 検査せず** `CMBlockBufferGetDataPointer` の第 1 引数に渡している。

## 問題点

1. **`data_buffer == NULL` のとき**に `CMBlockBufferGetDataPointer` が呼ばれる可能性がある。
2. 後続の **`Error::check(status, "CMBlockBufferGetDataPointer")`** は、OS が **status だけで失敗を返し、不正な第 1 引数でも安全に失敗する**とは限らない。実装・バージョンによっては **クラッシュ**の余地が残る。
3. この問題は **0010**（`from_raw_parts` 側のポインタ・長さ）とは独立しているが、**同じ関数内**で連続して発生するため、**0009 ではブロックバッファの存在を保証し、0010 ではポインタと長さの Rust 側前提を満たす**、という分担が分かりやすい。

## 再現性・テストについて

- 実機の Video Toolbox から **意図的に NULL を返すサンプル**を安定再現するのは難しい場合がある。
- それでも **防御的コード**（`data_buffer.is_null()` ならログして return）を入れる価値は、**将来の OS 差**や **異常系**に対する保険になる。

## 望ましい対応の方向（案）

1. **`CMSampleBufferGetDataBuffer` の直後**に `data_buffer.is_null()`（または `data_buffer == std::ptr::null_mut()`）を判定し、NULL のときは **`log::error!`** 等で記録し、**早期 return**（このコールバックではエンコード済みフレームを送らない）する。
2. Apple のドキュメントまたはヘッダで、**いつ NULL が返るか**を確認し、**コメントで根拠**を残す（draft 由来の場合は AGENTS のルールに従う）。
3. 単体テストでモックが難しい場合は、**レビューとコメント**で「NULL のときは送らない」という契約を明示する。

## 関連 issue

- **0010**（`from_raw_parts` と `data_pointer` / `data_pointer_len`）: ブロックバッファが非 NULL でも、ポインタと長さの組み合わせは別途検証が必要。

## 解決方法

`Encoder::process_encoded_output` 内で、`CMSampleBufferGetDataBuffer` の直後に `data_buffer.is_null()` を判定し、NULL のときは英語の `log::error!` を出して早期 return するようにした。

## 解決方法（再対応）

キーフレーム時に `CMSampleBufferGetFormatDescription` が NULL の場合はログして打ち切る。`CMBlockBufferGetDataLength` と `CMBlockBufferGetDataPointer` の長さを照合し、長さがブロック全体を超える場合はログして打ち切る。
