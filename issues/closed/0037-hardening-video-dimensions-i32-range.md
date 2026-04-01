# 映像の幅・高さを `u32` から `i32` に渡す経路で、負の寸法になり得る（エンコーダとデコーダ）

Created: 2026-04-01  
Completed: 2026-04-01  
Model: GPT-5.2

## なぜこの対応が必要か

Video Toolbox / CoreMedia に **`width` / `height` を `i32` で渡している**箇所が **`src/lib.rs` に複数ある**。いずれも元データは **`u32`**（または `DecoderCodec::Vp9` / `Av1` の `width` / `height`）であり、Rust の **`u32 as i32` の無検査キャスト**では、`u32 > i32::MAX` のとき **負の `i32` にラップ**する（例: `0x8000_0000u32 as i32` は負）。その結果 **負の寸法**が C API に渡りうる。挙動は **未定義に近い**（エラー・無視・内部不整合など実装依存）。

**エンコーダ経路**と **デコーダ経路（VP9 / AV1 のフォーマット生成）は別 API だが、**同じ根（寸法の範囲）**である。一方だけ直して **完了**とみなすと、**もう一方の経路が取り残される**。

## 現状

### エンコーダ

- **場所**: `Encoder::create_compression_session` 内の `VTCompressionSessionCreate`（`config.width` / `config.height` を `i32` にキャスト）。
- **検証**: `Encoder::validate_config` は **`width == 0` / `height == 0` のみ**拒否。`i32::MAX` 以下であることは **検証していない**。

### デコーダ（VP9 / AV1）

- **場所**: `Decoder::create_format_description` 内の `CMVideoFormatDescriptionCreate`（`DecoderCodec::Vp9` / `Av1` の `width` / `height` を `i32` にキャスト）。
- **検証**: **エンコーダの `validate_config` はここには効かない**。現状、寸法の **上限は検証していない**。

## 望ましい対応の方向（案）

- **エンコーダ**: `validate_config`（または同等の単一の検証関数）で、`width` / `height` が **`1 ..= i32::MAX as u32`** に収まることを要求し、範囲外は `InvalidConfig` で拒否する。
- **デコーダ**: `Decoder::new` / `create_format_description` の呼び出し前に、**`DecoderCodec::Vp9` / `Av1` の寸法**について同じ境界を満たすことを検証し、満たさない場合は `InvalidConfig`（または既存のエラー型で一貫した理由）で拒否する。**エンコーダと同じ上限ポリシー**に揃えること。
- 公開 API の型を変えず、**キャスト前に `try_into::<i32>()` 等で拒否**してもよい。

## 完了条件

- **負の寸法が C API に渡りうる経路**を、**エンコーダとデコーダ（VP9 / AV1）の両方**でコード上に残さない（ドキュメントのみは不可）。
- **エンコーダのみ**、または **デコーダのみ**の修正だけでは **未完了**とする。
- 可能なら **単体テスト**で境界付近（`i32::MAX as u32` は許容、`i32::MAX as u32 + 1` は拒否など）を、**両経路または共通ヘルパ経由**で検証する。

## 解決方法

- `validate_video_dimensions_for_toolbox` を追加し、`Encoder::validate_config` と `Decoder::create_format_description` の `Vp9` / `Av1` 分岐で共通検証する。
- 単体テスト `encoder_rejects_width_above_i32_max` / `encoder_rejects_height_above_i32_max` / `decoder_vp9_rejects_width_above_i32_max` / `decoder_av1_rejects_height_above_i32_max` を追加した。
