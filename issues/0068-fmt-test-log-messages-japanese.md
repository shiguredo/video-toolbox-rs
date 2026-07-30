# テストのログメッセージを日本語に統一する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-test-log-messages-japanese
- Polished: {YYYY-MM-DD}

## 目的

AGENTS.md は「テストのログメッセージは全て日本語にすること」と定めているが、`tests/test_encoder.rs` と `tests/test_decoder.rs` の `expect` / `panic!` / `assert!` メッセージが英語で書かれている。一方 `tests/test_codec_info.rs` は日本語で、ファイル間で不整合がある。

## 現状

- `tests/test_encoder.rs`: `expect("encoder construction must succeed")` / `expect("results mutex poisoned")` / `panic!("unexpected encode callback error: {e}")` 等が英語
- `tests/test_decoder.rs`: 同様に英語
- `tests/test_codec_info.rs`: `expect("H.264 のコーデック情報が返ってくること")` 等で日本語
- `src/encoder.rs` の `#[cfg(test)] mod tests`: `expect_err("rescale should overflow")` が英語

## 完了条件

全テストファイルの `expect` / `panic!` / `assert!` メッセージが日本語に統一されていること。
