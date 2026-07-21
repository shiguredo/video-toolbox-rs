# `src/encoder.rs` (1677 行) のモジュール分割を検討する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/change-split-encoder-module

## 目的

`src/encoder.rs` が 1677 行に達しており、CLAUDE.md の「テストが長くなるのはモジュール自体が大きすぎるサイン → `src/<module>.rs` 側の分割を検討する」目安に該当する。1 ファイルに以下の責務が同居している:

1. 公開型定義 (`EncoderConfig` / `ReconfigureParams` / `EncodeOptions` / `DataRateLimit` / `H264EncoderConfig` / `HevcEncoderConfig` / `CodecConfig` / `FrameData` / `EncodedFrame` / `EncodeHandler` trait / `FnEncodeHandler`)
2. バリデーション (`validate_config` / `validate_reconfigure_params` / `validate_average_bitrate` / `validate_fps_numerator` / `validate_expected_frame_rate` / `validate_data_rate_limits`)
3. `Encoder<H>` の lifecycle (`new` / `reconfigure` / `finish` / `Drop`)
4. FFI セッション生成 (`create_compression_session` / `add_common_properties` / `add_h264_specific_properties` / `add_h265_specific_properties` / `push_data_rate_limits_property`)
5. CVPixelBuffer 操作 (`copy_plane` / `validate_frame_data`)
6. 出力コールバック (`output_callback_h264` / `output_callback_h265` / 各種パラメータセット抽出)

横に伸びすぎていて、reconfigure / 検証 / FFI のどこを触っているのか即座に把握しにくい。

## 優先度根拠

- 機能には影響しない構造改善
- 直近の reconfigure 追加で更に肥大化したが、本格的な API 変更ではないため緊急度は低い
- ただし、将来 `DataRateLimits` などの動的更新項目追加 / `H264` 以外のコーデック追加で更に肥大化する見込みなので、いずれ対応すべき
- 後方互換のない変更 (モジュール構造変更 = 公開 API の場所が変わる可能性) なので Low + change prefix

## 現状

`src/encoder.rs` の構造 (主要シンボル):

- `H264Profile` / `H264EntropyMode` / `H264EncoderConfig`
- `HevcProfile` / `HevcEncoderConfig`
- `CodecConfig`
- `EncoderConfig`
- `DataRateLimit`
- `EncodeOptions`
- `ReconfigureParams`
- `validate_average_bitrate` / `validate_data_rate_limits` / `validate_fps_numerator` / `validate_expected_frame_rate`
- `push_data_rate_limits_property` (フリー関数)
- `FrameData<'a>`
- `EncodedFrame<T>`
- `EncodeHandler` trait + `FnEncodeHandler`
- `Encoder<H>` 構造体 + impl ブロック (約 1070 行)
  - `new` / `config` / `reconfigure` / `finish` / `Drop`
  - `create_compression_session`
  - `add_common_properties` / `add_h264_specific_properties` / `add_h265_specific_properties`
  - `validate_config` / `validate_reconfigure_params`
  - `copy_plane` / `validate_frame_data` / `frame_byte_len_checked`
  - `encode` / `encode_pixel_buffer`
  - `output_callback_h264` / `output_callback_h265`
  - `extract_h264_params` / `extract_h265_params`
  - `process_encoded_output` / `take_user_data` / `callback_from_ref_con` / `invoke_callback`
- `is_keyframe` / `vec_u8_from_raw_parts_safe` (フリー関数)
- `MAX_PARAMETER_SET_COPY_BYTES` / `MAX_ENCODED_BLOCK_COPY_BYTES` (定数)
- `#[cfg(test)] mod tests`

## 設計方針

`src/encoder.rs` をディレクトリモジュール `src/encoder/` に展開し、以下に分割する。

```
src/encoder/
├── mod.rs              # re-export と Encoder<H> 構造体本体 (new / reconfigure / encode / finish / Drop)
├── config.rs           # EncoderConfig / ReconfigureParams / EncodeOptions / CodecConfig / H264EncoderConfig / HevcEncoderConfig 等
├── handler.rs          # EncodeHandler trait / FnEncodeHandler
├── frame.rs            # FrameData / EncodedFrame
├── validation.rs       # validate_* ヘルパー
├── session.rs          # FFI セッション生成 (create_compression_session、add_*_properties)
├── pixel_buffer.rs     # CVPixelBuffer 操作 (copy_plane、validate_frame_data 等)
└── callback.rs         # 出力コールバック + パラメータセット抽出
```

公開 API の出力先 (`src/lib.rs` の `pub use encoder::{...}`) は変更しない。`use shiguredo_video_toolbox::Encoder` 等の外部呼び出しは壊さない。

CLAUDE.md の「`src/<module>/` のようにディレクトリモジュールの場合は `pbt/tests/prop_<module>/main.rs` にサブモジュール対応で分割する」に従い、PBT を導入する際は対応する分割を行う (本 issue では PBT までは扱わない)。

`#[cfg(test)] mod tests` は `mod.rs` に残す (Encoder 本体の private フィールドへのアクセスが必要なため)。

## 完了条件

- `src/encoder.rs` がディレクトリモジュール `src/encoder/` に展開されている
- 公開 API は変更されない (`use shiguredo_video_toolbox::*` で取れるシンボルが同じ)
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る
- 各サブモジュールが 400 行を超えない (目安)

## 解決方法

設計判断が必要な項目:

- `pub(crate)` のサブモジュール境界をどこに引くか
- 内部テストモジュールの配置先
- `Encoder<H>` の impl ブロックをどう分割するか (1 つの impl ブロックを複数ファイルに分けることは Rust では可能)

上記の判断は 0044-0052 の他 issue とは独立して進められるため、本 issue は他 issue とは衝突しないタイミングで対応する。場合によっては `issues/pending/` への移動も検討する。
