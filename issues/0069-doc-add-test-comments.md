# テスト関数にコメントを追加する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-test-comments
- Polished: {YYYY-MM-DD}

## 目的

AGENTS.md は「テストはコメントを重視すること」と定めているが、`tests/test_encoder.rs` の全 27 テスト関数のうち関数レベルのコメントがあるのは 0 件、`tests/test_decoder.rs` は 6 件中 1 件のみ。特に `h264_decoder` / `h265_decoder` のハードコードされた NAL ユニットの出自・前提条件はコメントなしでは保守不能。

## 現状

- `tests/test_encoder.rs`: ヘルパー関数（`synthetic_i420_frame` / `data_rate_limits_cap_windowed_output`）には詳細なコメントがあるが、`#[test]` 関数自体にはコメントがない
- `tests/test_decoder.rs`: `init_av1_decoder` のみコメントあり
- `tests/test_error.rs`: 2 件中 1 件のみ

## 完了条件

全 `#[test]` 関数に「何を検証するか・なぜその値を使うか」を日本語コメントで追記されていること。
