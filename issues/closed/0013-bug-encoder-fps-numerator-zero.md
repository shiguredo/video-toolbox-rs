# `fps_numerator == 0` と `CMTimeMake` の組み合わせが検証されていない

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`Encoder::validate_config` は `fps_denominator == 0` のみ拒否している。一方、`VTCompressionSessionEncodeFrame` および `encode_pixel_buffer` では `CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32)` を渡している。`timescale` に相当する第 2 引数が **0** になると、CoreMedia / Video Toolbox の時間表現として不正になり、**C 側の未定義動作やゼロ除算に相当する処理**のリスクがある。Rust のパニック以前の問題である。

## 現状

- **場所**: `src/lib.rs` の `Encoder::validate_config`、`encode`、`encode_pixel_buffer`
- `fps_denominator != 0` のみ保証。`fps_numerator == 0` は許容される。

## 問題

- `CMTimeMake` に `timescale` 0 を渡すことの安全性がコード上保証されていない。

## 望ましい対応の方向（案）

- `validate_config` で `fps_numerator == 0` を `InvalidConfig` 等で拒否する（または意味のある最小値に制約する）。
- `add_common_properties` の `fps_numerator.div_ceil(fps_denominator)` との整合も確認する。

## 解決方法

`Encoder::validate_config` で `fps_numerator == 0` のとき `Error::InvalidConfig { field: "fps_numerator", reason: "must not be zero" }` を返すようにした。`encoder_rejects_zero_fps_numerator` 単体テストを追加した。

## 解決方法（再対応）

`validate_config` で `width` / `height` が 0 の場合も `InvalidConfig` で拒否する。`validate_frame_data` の乗算は 0016 と合わせて `frame_byte_len_checked`（`checked_mul`）で扱う。
