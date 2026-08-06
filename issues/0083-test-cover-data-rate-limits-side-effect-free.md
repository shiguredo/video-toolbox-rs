# `reconfigure` の data_rate_limits のみ更新時に bitrate / fps が不変であることを検証するテストを追加する

- Priority: Low
- Created: 2026-08-06
- Completed:
- Model: deepseek-v4-flash
- Branch: feature/add-data-rate-limits-side-effect-free-test
- Polished: {YYYY-MM-DD}

## 目的

`ReconfigureParams` の 3 フィールドのうち、片肺更新（単独更新）で「対側が不変」を検証しているのは `average_bitrate` のみ更新と `expected_frame_rate` のみ更新の 2 パターンのみで、`data_rate_limits` のみ更新は反映と解除しか検証していない。3 パターン目を追加して網羅する。

## 現状

- `tests/test_encoder.rs` の `reconfigure_updates_data_rate_limits` は `data_rate_limits` の反映（`Some(limits)`）と解除（`None`）のみを assert している
- 更新後に `average_bitrate` / `fps_numerator` / `fps_denominator` が不変であることは検証していない
- `reconfigure_updates_only_average_bitrate` / `reconfigure_updates_only_expected_frame_rate` は対側不変を検証済みだが、`data_rate_limits` のみ更新の組み合わせは未カバー

## 設計方針

`reconfigure_updates_data_rate_limits` に、更新前の `average_bitrate` / `fps_numerator` / `fps_denominator` を保持して更新後に等しいことを assert する処理を追加する。`data_rate_limits` の反映・解除の検証は既存のまま維持する。

## 完了条件

- `reconfigure_updates_data_rate_limits`（または同等のテスト）が更新後に `average_bitrate` / `fps_numerator` / `fps_denominator` の不変を検証している
- `data_rate_limits` の反映・解除の検証が維持されている
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る
