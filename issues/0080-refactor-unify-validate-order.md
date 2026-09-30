# `validate_config` と `validate_reconfigure_params` の検証順序を統一する

- Priority: Low
- Created: 2026-08-01
- Completed: 2026-10-01
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

`src/encoder/validation.rs` でフレームレートと bitrate の検証を共通関数 `validate_frame_rate_and_bitrate` に集約し、`Encoder::validate_config` と `Encoder::validate_reconfigure_params` の両方がこの関数を呼ぶようにした。順序の一致をコメントで説明するのではなく、順序が 1 か所でしか決まらない構造にすることで、片方の経路だけを変更して順序が逆転する状態を作れなくした。

- 共通関数はフレームレートを先に、`average_bitrate` を次に検証する。`None` の項目は検証しない
- フレームレート側の検証は `frame_rate_validator` として呼び出し側から受け取る。エラーの `field` と上限超過の reason は公開エラーの一部であり、構築は `fps_numerator` と `"must fit in i32 for CMTime timescale"`、再設定は `expected_frame_rate` と `"must fit in i32 for CFNumber"` を報告する既存の挙動を維持する必要があるため
- ゼロ拒否の reason (`"must not be zero"`) と i32 上限の判定は既存の `validate_positive_i32_field` を両経路で共通して使う
- 検証順序を固定するテストを `tests/test_encoder.rs` に 2 つ追加した。`encoder_rejects_prioritizing_fps_error_over_bitrate_error` は `fps_numerator` と `average_bitrate` を同時に不正にした `Encoder::new` が `fps_numerator` のエラーを返すこと、`reconfigure_rejects_prioritizing_fps_error_over_bitrate_error` は `expected_frame_rate` と `average_bitrate` を同時に不正にした `reconfigure` が `expected_frame_rate` のエラーを返すことを検証する
- 追加したテストが順序逆転を実際に検出することを、共通関数内の 2 つの検証ブロックを入れ替えると 2 テストとも失敗することで確認した。既存のフィールド単位の拒否テストはエラー種別しか見ていないため、この入れ替えを検出できない
- `CHANGES.md` の `## develop` の `### misc` に `[UPDATE]` エントリを追記した
- `cargo test --workspace -- --test-threads=1` (59 テスト) / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通ることを確認した

なお「現状」と「完了条件」にある `validate_reconfigure_params` の `data_rate_limits` 検証は、`data_rate_limits` が構築時専用になって `ReconfigureParams` から削除されたため対象外とした。共通化したのはフレームレートと bitrate の検証順序のみである。
