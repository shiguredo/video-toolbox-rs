# `encode` で `CVPixelBuffer` をロックしたまま `VTCompressionSessionEncodeFrame` している

Created: 2026-04-01  
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

## レビュー（第 1 回）— ソース照合

- `encode` は `CVPixelBufferLockBaseAddress` のフラグ `0`（読み書き）でロックし、`copy_plane` のあと同一スコープで `VTCompressionSessionEncodeFrame` を呼ぶ。`CvPixelBufferUnlockGuard` の `Drop` は `encode` の `unsafe` ブロック終了時、つまり **`VTCompressionSessionEncodeFrame` の成功・失敗後**に走る。事実として「エンコード完了までロック保持」はコード上明確。
- `encode_pixel_buffer` は外部バッファを渡す経路であり、本 issue の「自前 `CVPixelBufferCreate`」とは別経路。対応方針を決めるとき **両経路のロック状態**を分けて書くと誤解が減る。

## レビュー（第 2 回）— Apple 文書との突き合わせ観点

- 公式が「`VTCompressionSessionEncodeFrame` 呼び出し時点でベースアドレスがロックされていてはならない」と明言しているか、**一次情報で確認**が必要。検索結果の要約だけに頼らないこと。
- `CVPixelBuffer` のロックは **CPU がメモリマップを触るため**のものであり、ハードウェアがテクスチャを読む経路とは別、という説明がドキュメントにあるかも併記候補。

## レビュー（第 3 回）— 失敗パス

- `copy_plane` が `Err` のとき `VTCompressionSessionEncodeFrame` は呼ばれず、`Drop` でアンロックされる。失敗パスは一貫している。
- `VTCompressionSessionEncodeFrame` が `Err` を返した場合も同様にアンロックされる。ロック漏れの懸念は `Guard` で潰せている。

## レビュー（第 4 回）— パフォーマンス

- ロック保持が長いほど、他の CPU アクセスと競合しうる。パフォーマンス回帰は **計測なしでは断定しない**が、対応後にマイクロベンチを取る価値はある。

## レビュー（第 5 回）— 代替実装

- 「`copy_plane` 成功直後に一度 `Unlock`、その後 `EncodeFrame`、失敗時は再ロック不要」の形は、API が許すか次第。二重アンロックを避ける `Guard` の組み替えが必要。
- 「`EncodeFrame` 前のみ明示アンロックし、`Guard` は `copy` 区間だけ」に短縮する案は、`Drop` の順序と二重解放に注意。

## レビュー（第 6 回）— 完了条件の検証可能性

- 「追試可能」とするなら、**参照した Apple ドキュメントの URL またはドキュメント名**を issue 完了時の PR に残すとよい（リンク切れリスクは注記）。

## レビュー（第 7 回）— テスト

- 単体テストでロック規約を直接は検証しにくい。**実機でのエンコード成功**が事実上の受け入れ条件になりやすい。回帰用に最小解像度・1 フレームのスモークを CI に含めるかは別論。

## レビュー（第 8 回）— 関連する別論点

- `fps_numerator as i32` の timescale は別 issue で扱った。ロック問題の修正と同一 PR に混ぜない方がレビューが楽。

## レビュー（第 9 回）— 将来の SDK

- Apple が将来 `CVPixelBuffer` のロック意味を変えた場合、コメントだけでは足りなくなる。**バージョン番号付きの注意**は過剰かもしれないが、CI の Xcode バージョン固定は有効。

## レビュー（第 10 回）— 締めチェックリスト

- [ ] 一次情報でロックと `VTCompressionSessionEncodeFrame` の関係を確認したか。
- [ ] `encode` と `encode_pixel_buffer` のどちらに効くか説明したか。
- [ ] 失敗パスでアンロックが保証されることを再確認したか。
- [ ] ドキュメントまたはコメントに根拠を残したか。
