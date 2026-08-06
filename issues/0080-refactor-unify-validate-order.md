# `validate_config` と `validate_reconfigure_params` の検証順序を統一する

- Priority: Low
- Created: 2026-08-01
- Completed:
- Model: Opus 4.7
- Branch: feature/refactor-unify-validate-order
- Polished: {YYYY-MM-DD}

## 目的

`src/encoder.rs` の `Encoder::validate_config` は fps 検証 (`fps_numerator`) を bitrate 検証より先に行うのに対し、`Encoder::validate_reconfigure_params` は bitrate 検証 (`average_bitrate`) を fps 検証 (`expected_frame_rate`) より先に行っており、検証順序が 2 経路で逆転している。両フィールドを同時に不正にした場合、`Encoder::new` では fps のエラーが、`Encoder::reconfigure` では bitrate のエラーが先に報告され、ユーザーが見るエラーが経路によって変わる。fps / bitrate の検証ロジックを共通関数化した際に見つかった不整合であり、検証順序も揃えておく。

## 現状

- `Encoder::validate_config` (src/encoder.rs): `fps_denominator` のゼロ拒否 → `fps_numerator` の検証 → `average_bitrate` の検証 → `data_rate_limits` の検証 → `max_key_frame_interval` / `max_frame_delay_count` の上限検証
- `Encoder::validate_reconfigure_params` (src/encoder.rs): `average_bitrate` の検証 → `expected_frame_rate` の検証 → `data_rate_limits` の検証

fps と bitrate の検証順序が経路ごとに異なる。

## 設計方針

`validate_config` に合わせ、fps 検証を bitrate 検証より先に実行する形に `validate_reconfigure_params` の検証順序を揃える。エラー優先順位を `Encoder::new` と `Encoder::reconfigure` で一貫させる。

## 関連 issue

- issue 0046（fps 検証の共通関数化）のレビューで発見した。`validate_positive_i32_field` の呼び出し順序を入れ替えるだけの変更

## 完了条件

- `Encoder::validate_reconfigure_params` の検証順序が `expected_frame_rate` → `average_bitrate` → `data_rate_limits` の順になっている
- `Encoder::new` と `Encoder::reconfigure` の両方で、fps と bitrate を同時に不正にした場合に fps のエラーが先に返る
- 検証順序を検証するテストが追加されている
- `CHANGES.md` の `## develop` に `[UPDATE]` としてリファクタリングのエントリを追記する（`### misc` サブセクション）
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -- --test-threads=1` が通る

## 解決方法

- `validate_reconfigure_params` 内の `average_bitrate` 検証と `expected_frame_rate` 検証の順序を入れ替える
- 既存テストはフィールド単位の検証のため無変更で通るはずだが、検証順序を固定するテストを追加する
