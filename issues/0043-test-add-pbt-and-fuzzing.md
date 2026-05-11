# PBT と Fuzzing の導入

Created: 2026-05-11
Model: deepseek-v4-pro

## 概要

`pbt/` ディレクトリと `fuzz/` ディレクトリが存在せず、CLAUDE.md のテスト規約に反している。
変更行数 2205 行に対して PBT も Fuzzing も 1 件も追加されていない。

## 背景

CLAUDE.md のテスト規約:

- `PBT(Property-Based Testing) や Fuzzing でテストを行うこと` (143 行)
- `PBT は proptest を使うこと` (144 行)
- `Fuzzing は cargo-fuzz を使うこと` (145 行)
- `PBT: 型情報（Strategy）に基づいて入力を生成し、プロパティを検証する（ラウンドトリップ等）` (90 行)
- `Fuzzing: 任意入力に対するクラッシュ耐性（パニック安全性）` (91 行)

## 対応方針

### PBT

`proptest` を dev-dependency に追加し、以下を作成する:

1. `pbt/tests/prop_decoder.rs`: `AvccNaluIter` のラウンドトリップ PBT
   - `pack_nalu_len4` で生成 → `AvccNaluIter` でパース → 元の NAL リストと一致
2. `pbt/tests/prop_decoder.rs`: `detect_h264_change` / `detect_hevc_change` のプロパティ
   - 同一パラメータセットなら必ず `None`
   - 異なるパラメータセットでストリーム内に出現するなら `Some`
3. `pbt/tests/prop_encoder.rs`: `Encoder::reconfigure` の前後でエンコード継続のプロパティ

### Fuzzing

`cargo-fuzz` を導入し、以下を作成する:

1. `fuzz/fuzz_targets/avcc_nalu_iter.rs`: 任意バイト列 → `AvccNaluIter` でパニックしない
2. `fuzz/fuzz_targets/detect_h264_change.rs`: 任意バイト列 → `detect_h264_change` でパニックしない
3. `fuzz/fuzz_targets/detect_hevc_change.rs`: 任意バイト列 → `detect_hevc_change` でパニックしない

## 変更対象ファイル

- `Cargo.toml`: `proptest` dev-dependency 追加、`cargo-fuzz` の設定
- `pbt/tests/prop_decoder.rs`: 新規作成
- `pbt/tests/prop_encoder.rs`: 新規作成（既存があれば追記）
- `fuzz/fuzz_targets/avcc_nalu_iter.rs`: 新規作成
- `fuzz/fuzz_targets/detect_h264_change.rs`: 新規作成
- `fuzz/fuzz_targets/detect_hevc_change.rs`: 新規作成
- `fuzz/Cargo.toml`: 新規作成

## 注意点

- PBT で「任意入力でパニックしないこと」は書かない（Fuzzing の役割）
- PBT と単体テストの重複を避ける (PBT でカバーできるものは単体テストで書かない)
