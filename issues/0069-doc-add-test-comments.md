# テスト関数にコメントを追加する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-test-comments
- Polished: 2026-08-01

## 目的

AGENTS.md は「テストはコメントを重視すること」と定めているが、`tests/test_encoder.rs` の全 31 テスト関数のうち関数直上のコメントがあるのは 0 件、`tests/test_decoder.rs` は 6 件中 0 件、`tests/test_error.rs` は 2 件中 0 件。特に `h264_decoder` / `h265_decoder` のハードコードされた NAL ユニットの出自・前提条件はコメントなしでは保守不能。

## 現状

- `tests/test_encoder.rs`: ヘルパー関数（`synthetic_i420_frame` / `data_rate_limits_cap_windowed_output`）には詳細な doc コメントがあるが、`#[test]` 関数の直上にコメントがない（関数本文のインラインコメントは一部にある）
- `tests/test_decoder.rs`: `#[test]` 関数の直上にコメントがない（`vp9_decoder` 等の本文にインラインコメントは一部ある）
- `tests/test_error.rs`: 関数直上のコメントは 0 件（`error_display_unknown_pixel_format` の本文冒頭にインラインコメントはある）
- `tests/test_codec_info.rs`: 1 件の `#[test]` 関数に詳細なコメントがあり、対象外

## 設計方針

- 各 `#[test]` 関数の直上に日本語の doc コメント（`///`）を付ける（`test_encoder.rs` / `test_decoder.rs` の既存ヘルパー関数が `///` を使用しており、`test_error.rs` も同じ形式に揃える）
- 内容は「何を検証するか・なぜその値を使うか」。特に `h264_decoder` / `h265_decoder` のハードコードされた NAL ユニットの出自・前提条件、解像度・フレーム数などの定数の意味を記す
- 既に本文にインラインコメントがある関数は、それを活かして doc コメントを補う

### 検証方法

各 `#[test]` 関数の直上に日本語の doc コメントがあり、その内容が「何を検証するか・なぜその値を使うか」の両方を満たすことを確認する。

## 完了条件

- `tests/test_encoder.rs` / `tests/test_decoder.rs` / `tests/test_error.rs` の全 `#[test]` 関数に「何を検証するか・なぜその値を使うか」を日本語の doc コメント（`///`）で追記されていること
- `tests/test_codec_info.rs` は対象外（関数直上のコメントはないが、本文に詳細なコメントがあり「何を検証するか」が既に記述されている）
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0068: `tests/test_encoder.rs` / `tests/test_decoder.rs` を変更対象とし、同一ファイルを変更する。目的は別のため重複ではないが、差分衝突に注意する
- issue 0051: `tests/test_encoder.rs` を変更対象とし、同一のテスト関数に触れる
