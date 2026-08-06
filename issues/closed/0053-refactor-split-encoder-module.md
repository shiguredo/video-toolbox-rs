# `src/encoder.rs` のモジュール分割を検討する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed: 2026-08-06
- Model: Opus 4.7
- Branch: feature/refactor-split-encoder-module
- Polished: 2026-07-31

## 目的

`src/encoder.rs` が 1697 行に達しており、shiguredo-rust スキルの「テストが長くなるのはモジュール自体が大きすぎるサインなので `src/<module>.rs` 側の分割を検討すること」という目安に該当する。1 ファイルに以下の責務が同居している:

1. 公開型定義 (`EncoderConfig` / `ReconfigureParams` / `EncodeOptions` / `DataRateLimit` / `H264Profile` / `H264EntropyMode` / `HevcProfile` / `H264EncoderConfig` / `HevcEncoderConfig` / `CodecConfig` / `FrameData` / `EncodedFrame` / `EncodeHandler` trait / `FnEncodeHandler`)
2. バリデーション (`validate_config` / `validate_reconfigure_params` / `validate_average_bitrate` / `validate_fps_numerator` / `validate_expected_frame_rate` / `validate_data_rate_limits`)
3. `Encoder<H>` の lifecycle (`new` / `config` / `reconfigure` / `finish` / `Drop`)
4. FFI セッション生成 (`create_compression_session` / `add_common_properties` / `add_h264_specific_properties` / `add_h265_specific_properties` / `push_data_rate_limits_property`)
5. CVPixelBuffer 操作とフレーム送信 (`copy_plane` / `validate_frame_data` / `encode` / `encode_pixel_buffer`)
6. 出力コールバック (`output_callback_h264` / `output_callback_h265` / 各種パラメータセット抽出)

横に伸びすぎていて、reconfigure / 検証 / FFI のどこを触っているのか即座に把握しにくい。

## 優先度根拠

- 機能には影響しない構造改善
- 直近の reconfigure 追加で更に肥大化したが、本格的な API 変更ではないため緊急度は低い
- `DataRateLimit` などの動的更新項目は追加済みであり、今後も動的更新項目追加 / `H264` 以外のコーデック追加で更に肥大化する見込みなので、いずれ対応すべき
- 本 issue は公開 API を変更しない（後方互換の変更はない）ため、リファクタリングとして扱う
- Low

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
- `Encoder<H>` 構造体 + impl ブロック (約 1100 行)
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
- `ParameterSets` (型エイリアス)
- `#[cfg(test)] mod tests`

## 設計方針

`src/encoder.rs` をディレクトリモジュール `src/encoder/` に展開し、以下に分割する。

```
src/encoder/
├── mod.rs              # re-export と Encoder<H> 構造体本体 (new / config / reconfigure / finish / Drop)
├── config.rs           # EncoderConfig / ReconfigureParams / EncodeOptions / CodecConfig / H264Profile / H264EntropyMode / HevcProfile / H264EncoderConfig / HevcEncoderConfig / DataRateLimit 等
├── handler.rs          # EncodeHandler trait / FnEncodeHandler
├── frame.rs            # FrameData / EncodedFrame
├── validation.rs       # validate_* ヘルパー (Encoder の関連関数として impl ブロックごと移動する validate_config / validate_reconfigure_params を含む)
├── session.rs          # FFI セッション生成 (create_compression_session、add_*_properties、push_data_rate_limits_property)
├── pixel_buffer.rs     # CVPixelBuffer 操作とフレーム送信 (copy_plane、validate_frame_data、frame_byte_len_checked、encode、encode_pixel_buffer)
└── callback.rs         # 出力コールバック + パラメータセット抽出 (output_callback_h264 / output_callback_h265 / process_encoded_output / take_user_data / callback_from_ref_con / invoke_callback / extract_h264_params / extract_h265_params / vec_u8_from_raw_parts_safe / is_keyframe / ParameterSets / MAX_PARAMETER_SET_COPY_BYTES / MAX_ENCODED_BLOCK_COPY_BYTES)
```

公開 API の出力先 (`src/lib.rs` の `pub use encoder::{...}`) は変更しない。`use shiguredo_video_toolbox::Encoder` 等の外部呼び出しは壊さない。

このため `mod.rs` で `pub use` による re-export を行う。shiguredo-rust スキルの「re-export は基本的にやらないこと」に対する例外であり、理由は「モジュール構造の内部変更（分割）で公開 API のパスを維持するため」とする。許可の根拠は `CODEBASE.md` に追記する（トレイト許可と同じ記録の慣行）。

`Encoder<H>` の impl ブロックは複数モジュールに分割してよい（Rust では同一型への impl ブロックを複数作って別モジュールに置くことができる。サブモジュールは親モジュールで定義された構造体の private フィールドにアクセスできる）。`unsafe impl Send for Encoder<H>` は `mod.rs` に残す。`#[cfg(test)] mod tests` は `mod.rs` に残す。

shiguredo-rust スキルの「`src/<module>/` のようにディレクトリモジュールの場合は `pbt/tests/prop_<module>/main.rs` にサブモジュール対応で分割する」に従い、PBT を導入する際は対応する分割を行う (本 issue では PBT までは扱わない)。

## 関連 issue

- issue 0041（Error 型の String 化）は `process_encoded_output` / `extract_h264_params` / `extract_h265_params` / `vec_u8_from_raw_parts_safe` 等の同じ関数群を対象とする。どちらを先に実施しても成立する（先に本 issue を実施する場合は、0041 の変更対象は分割後のモジュール（主に callback.rs）に移る）
- issue 0046（fps バリデータ統合）は `validate_fps_numerator` / `validate_expected_frame_rate` を統合する。どちらを先に実施しても成立する（先に本 issue を実施する場合は、統合対象は validation.rs 内の関数になる）
- issue 0047（プロパティ構築の共通化）は `add_common_properties` / `reconfigure` のプロパティ構築を共通化する。どちらを先に実施しても成立する（先に本 issue を実施する場合は、ヘルパーは session.rs に置かれる）

## 完了条件

- `src/encoder.rs` がディレクトリモジュール `src/encoder/` に展開されている
- 公開 API は変更されない (`use shiguredo_video_toolbox::*` で取れるシンボルが同じ)
- re-export の許可理由が `CODEBASE.md` に追記されている
- 各サブモジュールが実装コードのみで 400 行を超えない（目安。`#[cfg(test)]` のテストコードは行数に含めない）
- `CHANGES.md` の `## develop` に `[UPDATE]` としてリファクタリングのエントリを追記する（公開 API の変更を伴わないため `### misc` サブセクション）
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

設計判断が必要な項目:

- `pub(crate)` のサブモジュール境界をどこに引くか（分割スキームのとおり）。`mod.rs` に残る `new` / `reconfigure` から呼ばれる関数（`validate_config` / `validate_reconfigure_params` / `create_compression_session` / `push_data_rate_limits_property`）はサブモジュールへ移るため、`pub(crate)` 化が必要になる
- 内部テストモジュールの配置先（`mod.rs` に残す）
- `Encoder<H>` の impl ブロックをどう分割するか（同一型への impl ブロックを複数作って別モジュールに置く。`encode` / `encode_pixel_buffer` は `pixel_buffer.rs` の impl ブロックへ、`validate_config` / `validate_reconfigure_params` は `validation.rs` の impl ブロックへ移す）

本 issue は上記の関連 issue とは独立して進められる。pending にはしない（完了条件まで定義済みの実装 issue であり、仕様的に対応が難しい issue ではない）。

## 解決方法

`src/encoder.rs` (1743 行) をディレクトリモジュール `src/encoder/` に分割した。shiguredo-rust 規約の「mod.rs を使わないこと」に従い、親は `src/encoder.rs` のままサブモジュールを `src/encoder/` 配下に置く構成とした (issue の設計方針の `src/encoder/mod.rs` は規約に合わせて変更)。

- `src/encoder.rs`: モジュール宣言・re-export・`Encoder<H>` 構造体本体 (`new` / `config` / `reconfigure` / `finish`)・`Drop`・`Send`・内部テスト
- `src/encoder/config.rs`: 設定型 (`EncoderConfig` / `ReconfigureParams` / `EncodeOptions` / `CodecConfig` 等)
- `src/encoder/handler.rs`: `EncodeHandler` trait / `FnEncodeHandler`
- `src/encoder/frame.rs`: `FrameData` / `EncodedFrame`
- `src/encoder/validation.rs`: バリデーション (`validate_config` / `validate_reconfigure_params` 等)
- `src/encoder/session.rs`: FFI セッション生成とプロパティ構築
- `src/encoder/pixel_buffer.rs`: CVPixelBuffer 操作とフレーム送信 (`encode` / `encode_pixel_buffer`)
- `src/encoder/callback.rs`: 出力コールバックとパラメータセット抽出

`Encoder<H>` の impl ブロックは複数モジュールに分割し、親の `new` / `reconfigure` から呼ばれる関数は `pub(super)` 化した。公開 API のパスは不変 (`src/lib.rs` の `pub use encoder::{...}` は変更なし)。re-export の許可理由を `CODEBASE.md` に追記した。`CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記した。`cargo test --workspace` は全 46 テストがパスし、`cargo doc --no-deps` も警告なしで通る。
