# `Encoder::encode` で `copy_plane` が失敗したとき `CVPixelBufferUnlockBaseAddress` が呼ばれない

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`copy_plane` を `Result` にしたうえで `encode` 内で `?` により早期 return する経路ができた。`CVPixelBufferLockBaseAddress` 成功後は **`CVPixelBufferUnlockBaseAddress` と対になる必要**があるが、従来は手動アンロックが `copy_plane` の後にしかなく、**エラー時はアンロックされない**。その状態で `CfPtrMut` の `Drop` が `CFRelease` すると、**ロックしたまま解放**する経路になり得る。NULL / stride / オーバーフロー等の防御が発火したときに後始末が壊れる。

## 現状

- **場所**: `src/lib.rs` の `Encoder::encode`（`CVPixelBufferLockBaseAddress` 直後〜`copy_plane`）

## 問題

- `copy_plane` が `Err` のとき、`CVPixelBufferUnlockBaseAddress` が実行されない。

## 解決方法

`CVPixelBufferLockBaseAddress` 成功直後に **`CvPixelBufferUnlockGuard`** を束ね、`Drop` で必ず `CVPixelBufferUnlockBaseAddress` を呼ぶようにした。成功経路では従来どおりスコープ終了時にアンロックされ、**エラー経路でも `?` で抜ける前にガードが `Drop` する**。
