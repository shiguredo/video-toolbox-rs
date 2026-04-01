# `encode` で `CVPixelBuffer` をロックしたまま `VTCompressionSessionEncodeFrame` している

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`Encoder::encode` は `CVPixelBufferLockBaseAddress` のあと `CvPixelBufferUnlockGuard` で **`VTCompressionSessionEncodeFrame` の戻りまでロックを保持**している。従来の多くのサンプルは **CPU でピクセル書き込み後にアンロックしてから**エンコードに渡す。Apple の契約が「エンコード時はアンロック必須」である場合、**未定義動作・デッドロック・パフォーマンス劣化**のリスクがある。逆に、現状が正しいなら **レビューアが誤解しない根拠**（ドキュメントまたはコメント）が必要である。

## 現状

- **場所**: `src/lib.rs` の `Encoder::encode`（`CVPixelBufferLockBaseAddress` → `copy_plane` → `VTCompressionSessionEncodeFrame` → `Drop` でアンロック）

## 望ましい対応の方向（案）

- Apple ドキュメント・サンプルと照合し、**エンコード呼び出し時点でロックが許容されるか**を確定する。
- 許容されない場合は **`VTCompressionSessionEncodeFrame` の直前に明示アンロック**し、失敗パスでは引き続き `Drop` で確実にアンロックする形に整理する。
- 許容される場合は **なぜロックを跨いでよいか**をコードコメントで短く明記する。

## 解決の完了条件

- 上記の「契約に合致」または「意図的で根拠あり」のどちらかが **コードとドキュメントで追試可能**な状態になっていること。

## 解決方法

- `Encoder::encode` では `copy_plane` を囲む内側スコープのみ `CVPixelBufferLockBaseAddress` と `CvPixelBufferUnlockGuard` をかけ、`VTCompressionSessionEncodeFrame` はアンロック後に呼ぶように変更した（`src/lib.rs`）。
- `encode_pixel_buffer` はロックを取得しない旨を rustdoc に追記した。
