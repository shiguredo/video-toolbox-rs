# `validate_fps_numerator` と `validate_expected_frame_rate` を統合する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-merge-fps-validators

## 目的

`validate_fps_numerator` (`src/encoder.rs:234-248`) と `validate_expected_frame_rate` (`src/encoder.rs:251-265`) が同一ロジック (`u32` 値の「ゼロ拒否 + `i32::MAX` 上限拒否」) を持ち、差分はエラー時の `field` / `reason` 文字列のみ。コピペで増殖した状態を放置すると、将来 `i32` 上限境界の判定をひとつ変えるときに片方を直し忘れる典型的な保守事故が起きる。

## 優先度根拠

- 既に動作している同型ロジックの整理であり、緊急度は低い
- 将来 `DataRateLimits` 等の項目を追加するときに同じパターンの 3 関数目が増える可能性が高く、その前にやっておく価値がある
- Low 相当

## 現状

```rust
// src/encoder.rs:234-248
fn validate_fps_numerator(value: u32) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero",
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must fit in i32 for CMTime timescale",
        });
    }
    Ok(())
}

// src/encoder.rs:251-265
fn validate_expected_frame_rate(value: u32) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must not be zero",
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must fit in i32 for CFNumber",
        });
    }
    Ok(())
}
```

両者の違いは:

- `field` 文字列: `"fps_numerator"` vs `"expected_frame_rate"`
- 上限拒否時の `reason`: `"must fit in i32 for CMTime timescale"` vs `"must fit in i32 for CFNumber"`

`CMTimeMake` の timescale 用と `CFNumber` 経由の API 用で reason を分けたい意図はあるが、引数化すれば 1 関数で済む。

## 設計方針

`validate_positive_i32_field(field: &'static str, reason_overflow: &'static str, value: u32) -> Result<(), Error>` のような共通関数 1 本に統合する。あるいは `reason_overflow` をフィールドから推測したくないなら、呼び出し側で 2 引数 (`field`, `reason_overflow`) を必ず指定する方針にする。

エラー時の `reason: "must not be zero"` は両者共通なので関数内に閉じ込める。

## 完了条件

- `validate_fps_numerator` / `validate_expected_frame_rate` のいずれか 1 関数 (もしくは新名称の共通関数) のみが残っている
- 呼び出し側 (`Encoder::validate_config`、`Encoder::validate_reconfigure_params`) が新関数を使う形に書き換わっている
- 既存のテスト (`encoder_rejects_zero_fps_numerator`、`encoder_rejects_fps_numerator_above_i32_max`、`reconfigure_rejects_zero_expected_frame_rate`、`reconfigure_rejects_expected_frame_rate_above_i32_max`) が同じエラーメッセージで通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

候補:

```rust
fn validate_positive_i32_field(
    field: &'static str,
    reason_overflow: &'static str,
    value: u32,
) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field,
            reason: "must not be zero",
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field,
            reason: reason_overflow,
        });
    }
    Ok(())
}
```

呼び出し例:

```rust
validate_positive_i32_field("fps_numerator", "must fit in i32 for CMTime timescale", config.fps_numerator)?;
validate_positive_i32_field("expected_frame_rate", "must fit in i32 for CFNumber", fps)?;
```
