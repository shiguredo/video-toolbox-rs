# `expected_frame_rate` の上限エラー reason を実用途に合わせて修正する

- Priority: Low
- Created: 2026-08-01
- Completed:
- Model: Opus 4.7
- Branch: feature/refactor-clarify-expected-frame-rate-reason
- Polished: {YYYY-MM-DD}

## 目的

`src/encoder.rs` の `Encoder::validate_reconfigure_params` が `expected_frame_rate` の `i32::MAX` 上限超過時に返す `reason: "must fit in i32 for CFNumber"` は、実用途の一部しか述べていない。`reconfigure` で設定した `expected_frame_rate` は `self.config.fps_numerator` に代入され (src/encoder.rs の `Encoder::reconfigure`)、以後 `CMTimeMake` の timescale (src/encoder.rs の `Encoder::encode_pixel_buffer`) としても使われる。つまり CFNumber だけでなく CMTimeMake の timescale としても `i32` 制約が課される値である。同様に `fps_numerator` 側の `reason: "must fit in i32 for CMTime timescale"` も、`kVTCompressionPropertyKey_ExpectedFrameRate` の CFNumber 化 (src/encoder.rs の `Encoder::create_compression_session`) を述べておらず部分正確である。

## 現状

- `validate_positive_i32_field("expected_frame_rate", "must fit in i32 for CFNumber", fps)` (src/encoder.rs の `Encoder::validate_reconfigure_params`)
- `validate_positive_i32_field("fps_numerator", "must fit in i32 for CMTime timescale", config.fps_numerator)` (src/encoder.rs の `Encoder::validate_config`)
- 両方とも、値が実際に使われる経路 (CFNumber と CMTimeMake timescale の両方) を述べていない

## 設計方針

`expected_frame_rate` / `fps_numerator` の上限エラー reason を、両方の用途 (CMTimeMake の timescale と CFNumber) を述べる文面に揃える。例: `"must fit in i32 for CMTime timescale and CFNumber"`。

## 関連 issue

- issue 0051（テストアサーション強化）が `reason` 文字列の固定を予定しているため、reason 変更時は該当テストの期待値も追従させる

## 完了条件

- `expected_frame_rate` と `fps_numerator` の上限エラー reason が実用途 (CMTimeMake の timescale と CFNumber) を述べる文面になっている
- `tests/test_encoder.rs` の `reconfigure_rejects_expected_frame_rate_above_i32_max` 等、reason を検証しているテストの期待値が更新されている
- `CHANGES.md` の `## develop` に `[UPDATE]` としてエントリを追記する
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -- --test-threads=1` が通る

## 解決方法

- `validate_config` / `validate_reconfigure_params` の 2 箇所の `reason_overflow` 引数を変更する
- reason 文字列を検証しているテストの期待値を更新する
