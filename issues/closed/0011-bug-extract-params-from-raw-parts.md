# `extract_h264_params` / `extract_h265_params` の `from_raw_parts` が異常なポインタ・サイズの組み合わせで未定義動作になり得る

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

H.264 / H.265 のパラメータセット抽出で、`CMVideoFormatDescriptionGetH264ParameterSetAtIndex` 等の戻り値を `Error::check` しているだけで、取得した **ポインタとサイズの組み合わせ**に対する防御がない。`std::slice::from_raw_parts(ps_ptr, ps_size)` は、**NULL かつ正のサイズ**など異常な組み合わせで **未定義動作**になる。通常の API では起きにくいが、**フレームワークの異常時や将来の OS 差**で表に出る可能性がある。

## 現状

- **場所**: `src/lib.rs` の `Encoder::extract_h264_params` および `extract_h265_params`
- 各ステップで `Error::check` は成功しているが、`sps_ptr` / `pps_ptr` / `vps_ptr` と対応する `*_size` を検証せず `from_raw_parts` に渡している。

## 問題

- `ps_ptr.is_null() && ps_size > 0` のような場合に **UB**。
- サイズが実バッファより大きい場合も **UB**（API が保証する前提の確認が必要）。

## 望ましい対応の方向（案）

- `from_raw_parts` の前に、`ptr.is_null()` や `size == 0` の扱いを整理する（空の `Vec` を返す、エラーにする、など）。
- Apple のドキュメントで「成功時はポインタは非 NULL」等の保証があるか確認し、コメントに根拠を残す。

## 解決方法

`vec_u8_from_raw_parts_safe` を用いて `extract_h264_params` / `extract_h265_params` でパラメータセットを `Vec` にコピーするようにした。長さ 0 は空 `Vec`、長さ正で NULL ポインタは `None` を返して抽出を打ち切る。

## 解決方法（再対応）

`extract_h264_params` / `extract_h265_params` の先頭で `description` が NULL の場合はログして `None` を返す。
