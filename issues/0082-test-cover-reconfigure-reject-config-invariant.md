# `reconfigure` の拒否時に `config()` が不変であることを検証するテストを追加する

- Priority: Low
- Created: 2026-08-06
- Completed: 2026-10-02
- Model: deepseek-v4-flash
- Branch: feature/add-reconfigure-reject-config-invariant-test
- Polished: {YYYY-MM-DD}

## 目的

`Encoder::reconfigure` の拒否系テストはエラー種別のみを検証しており、「拒否時に `self.config` が変更されない」という契約がテスト的に裏付けられていない。この契約は `Encoder::reconfigure` の rustdoc に明記されているため、回帰テストで固定する。

## 現状

- `tests/test_encoder.rs` の `reconfigure_err` ヘルパーは `reconfigure` のエラーを返すだけで、`encoder.config()` の状態は検証していない
- `reconfigure_rejects_*` 系 7 テストはすべて `reconfigure_err` 経由でエラー種別のみを assert している
- `src/encoder.rs` の `Encoder::reconfigure` の rustdoc は「`VTSessionSetProperties` が失敗した場合は `self.config` を変更せず、セッションも生かしたままエラーを返す」と明記している
- 現実装は `validate_reconfigure_params` が先に実行されるため拒否時は `self` に触れないが、テスト的な裏付けは無い

## 設計方針

`reconfigure_err` ヘルパーを変更して、拒否後の `config()` の状態（`average_bitrate` / `fps_numerator` / `fps_denominator` / `data_rate_limits`）が呼び出し前の初期値と等しいことを検証する。ヘルパー経由の 7 テストすべてに適用されるため、1 箇所の変更で全拒否系テストの検証が強化される。

## 完了条件

- `reconfigure_err` ヘルパー（または同等の仕組み）が拒否後の `config()` の不変性を検証している
- `reconfigure_rejects_*` 系テストがすべて通過する
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` が通る

## 解決方法

`tests/test_encoder.rs` の `reconfigure_err` ヘルパーを変更し、`reconfigure` 呼び出しの前後で
`config()` の `average_bitrate` / `fps_numerator` / `fps_denominator` / `data_rate_limits` を
比較して、拒否時に設定が変更されないことを検証するようにした。拒否系テストはすべて同ヘルパーを
経由するため、エラー種別の検証に加えて設定の不変性が全ケースで確認される。

期待値は呼び出し前の `config()` から取得している。検証エラーは `validate_reconfigure_params` が
`self` に触れる前に返すため、呼び出し前の値がそのまま拒否後の期待値になる。

検証が実際に回帰を検出することは、`Encoder::reconfigure` の検証前に `self.config` を書き換える
回帰を一時注入し、`reconfigure_rejects_zero_bitrate` が
`拒否された reconfigure が average_bitrate を変更している` で失敗することを確認した。

なお、本文の「`reconfigure_rejects_*` 系 7 テスト」は実数と一致していなかった。
`tests/test_encoder.rs` 側の拒否系テストは 5 件で、同じ不変性は
`pbt/tests/prop_encoder.rs` の `encoder_reconfigure_rejects_invalid_params` が既に検証している。
