# `Encoder::Drop` で `sourceFrameRefCon` の `Box<T>` がリークする

Created: 2026-05-11
Model: deepseek-v4-pro

## 概要

`Encoder::Drop` 内で `self.finish()` → `VTCompressionSessionInvalidate` を即座に呼んでおり、pending フレームの `sourceFrameRefCon` (`Box::new(user_data)`) が解放されずにリークする。

## 背景

`finish()` が呼ぶ `VTCompressionSessionCompleteFrames` は「これ以上フレームは来ない」と伝えるだけで、エンコード完了とコールバック発火を**同期的に待たない**。
直後の `VTCompressionSessionInvalidate` が pending フレームを破棄し、`sourceFrameRefCon` として渡した `Box::new(user_data)` が VideoToolbox 側で解放されずリークする。

Apple の `VTCompressionSessionInvalidate` ドキュメント:
> "Pending frames are discarded, including their sourceFrameRefCon values."

`VTCompressionSessionCompleteFrames` のドキュメント:
> "output callback calls may occur at any time until VTCompressionSessionInvalidate is called."

すなわち `CompleteFrames` 呼び出し後もコールバックが遅延する可能性があり、その間に `Invalidate` すると破棄対象になる。破棄されたフレームの `sourceFrameRefCon` は VideoToolbox 側で解放されない（VT は任意のユーザーデータの解放方法を知らない）。

## 影響範囲

- `Encoder::new` で構築した `Encoder<T>` が `Drop` されるたびに、未出力の `sourceFrameRefCon` (`Box<T>`) がリークする。
- `T` にヒープ確保されたデータが含まれる場合、メモリリークの規模が大きくなる。

## 再現手順

1. `Encoder::new(config, callback)` でエンコーダーを作成する
2. `encoder.encode(...)` で数フレームエンコードする
3. `encoder.finish()` を呼ばずに `drop(encoder)` する
4. `sourceFrameRefCon` として渡した `Box<T>` が解放されない

## 対照: デコーダー側の正しい実装

`Decoder::finish()` (`src/decoder.rs:288-297`) は `VTDecompressionSessionFinishDelayedFrames` に加えて `VTDecompressionSessionWaitForAsynchronousFrames` を呼び、**同期的に完了を待つ**。その後 `Drop` で `Invalidate` している。

## 対応方針

`finish()` の内部または `Drop` において、全コールバック完了を待つ機構を導入する。

案 1: `Arc<AtomicUsize>` で pending カウントを管理し、`finish()` 後にカウントが 0 になるまで `Condvar` で待機してから `Invalidate` する。
案 2: `VTCompressionSessionCompleteFrames` に `kCMTimePositiveInfinity` 相当を渡せるか調査する。

## 変更対象ファイル

- `src/encoder.rs`: `finish()` の改修、または `Drop` の改修、pending カウンタ管理機構の追加

## 注意点

- デッドロック回避: コールバック内で `Encoder` を操作するとデッドロックの可能性がある。ドキュメントで注意喚起する。
- `finish()` 後に長時間コールバックを待つ可能性があるが、これは正常動作。
