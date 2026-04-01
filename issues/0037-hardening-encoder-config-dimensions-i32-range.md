# `EncoderConfig` の幅・高さを `i32` にキャストすると負の寸法になり得る

Created: 2026-04-01  
Model: GPT-5.2

## なぜこの対応が必要か

`VTCompressionSessionCreate` や `CMVideoFormatDescriptionCreate`（VP9 / AV1）には **`width` / `height` を `i32` で渡している**（`src/lib.rs`）。`EncoderConfig` の `width` / `height` は **`u32`** で、`validate_config` は **非ゼロのみ**検証し、**`i32::MAX` 以下であることは検証していない**。

Rust では **`u32` を `i32` に無検査キャスト**すると、`u32 > i32::MAX` の値は **負の `i32` にラップ**する（例: `0x8000_0000u32 as i32` は負）。その結果、Video Toolbox / CoreMedia に **負の寸法**が渡りうる。挙動は **未定義に近い**（エラーになる・無視される・内部不整合など実装依存）。

## 現状

- **場所**: `create_compression_session` の `VTCompressionSessionCreate` 引数、`create_format_description` の `DecoderCodec::Vp9` / `Av1` の `CMVideoFormatDescriptionCreate` 引数。
- **検証**: `validate_config` は `width == 0` / `height == 0` のみ拒否。

## 望ましい対応の方向（案）

- **`validate_config`（およびデコーダ側に同様の寸法があるなら同様）で、`width` / `height` が `1 ..= i32::MAX as u32` に収まることを要求**し、範囲外は `InvalidConfig` で拒否する。
- または **API を `u32` から変更せず**、**キャスト前に `try_into::<i32>()`** 等で拒否する（公開 API の互換を保ちつつ内部で統一）。

## 完了条件

- **負の寸法が C API に渡りうる経路**をコード上で潰す（ドキュメントのみは不可）。
- 可能なら **単体テスト**で境界付近（`i32::MAX as u32` とその直後）を検証する。
