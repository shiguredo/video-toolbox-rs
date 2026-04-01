# `next_input_pts` / `next_output_pts` の `i64` 加算がオーバーフローし得る

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`encode` / `encode_pixel_buffer` で `next_input_pts += self.config.fps_denominator as i64` を、`next_frame` で `next_output_pts += self.config.fps_denominator as i64` を行っている。`i64` の加算が **オーバーフロー**すると、Rust では **デバッグビルドではパニック**の対象になり得る（リリースではラップ）。極端に長いストリームや異常な設定の組み合わせでは理論上あり得る。

## 現状

- **場所**: `src/lib.rs` の `Encoder::encode`、`encode_pixel_buffer`、`next_frame`
- `+=` は `checked_add` 等を使っていない。

## 問題

- オーバーフロー時の挙動が「デバッグでパニック」「リリースで誤った PTS 」など、意図と異なる可能性がある。

## 望ましい対応の方向（案）

- `checked_add` で失敗時はエラーにする、または `saturating_add` で明示的に方針を決める。
- プロダクトとして「そこまで長いストリームは想定しない」なら、コメントで上限方針を記載する。

## 解決方法

`encode` / `encode_pixel_buffer` では `next_input_pts` を `checked_add` で更新し、失敗時は `Error::LimitExceeded` を返す。`next_frame` では `next_output_pts` を `checked_add` し、失敗時は英語の `log::error!` を出す。

## 解決方法（再対応）

`next_frame` の戻り値を `Result<Option<EncodedFrame>, Error>` とし、`next_output_pts` の `checked_add` 失敗時は `Error::LimitExceeded` を返す。`next_frame` のドキュメントに出力 PTS オーバーフロー時の挙動を追記した。

## 解決方法（レビュー追補）

追加レビューで、**`remove` より後に `checked_add` を `?` していた**と、オーバーフロー時に **`EncodedFrame` がドロップされエンコード済みデータを失う**不整合があったため修正した。

- `output_frames.contains_key(&self.next_output_pts)` で取出し対象があることを確認する。
- **`remove` より前に** `next_output_pts.checked_add(fps_denominator)` の可否を検証し、失敗時は `Err` のみ返す（マップ上のフレームは残す）。
- 検証成功後に `remove` し、`next_output_pts` を更新する。
- ドキュメントに、PTS オーバーフロー時も未取出しフレームは `output_frames` に残る旨を追記した。

## 解決方法（レビュー追補 2）

`next_frame` が先頭で `try_recv()` に失敗すると即 `Ok(None)` となり、**既に `output_frames` にバッファしたフレーム**（例: 先に `pts` が大きいフレームだけ届いた後に順序が埋まった場合）を返せない不具合があった。

- `try_recv()` は失敗しても続行し、**その後** `output_frames` に `next_output_pts` 一致分があるかを判定する。
- ドキュメントに、チャネルに新規がなくてもバッファから取出しうる旨を追記した。
