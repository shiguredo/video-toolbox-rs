# `validate_fps_numerator` と `validate_expected_frame_rate` を統合する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed: 2026-08-01
- Model: Opus 4.7
- Branch: feature/refactor-merge-fps-validators
- Polished: 2026-07-31

## 目的

`src/encoder.rs` の `validate_fps_numerator` と `validate_expected_frame_rate` が同一ロジック (`u32` 値の「ゼロ拒否 + `i32::MAX` 上限拒否」) を持ち、差分はエラー時の `field` / `reason` 文字列のみ。コピペで増殖した状態を放置すると、将来 `i32` 上限境界の判定をひとつ変えるときに片方を直し忘れる典型的な保守事故が起きる。

## 優先度根拠

- 既に動作している同型ロジックの整理であり、緊急度は低い
- 同型パターン（ゼロ拒否 + `i32::MAX` 上限拒否）が現状 2 関数に現れており、放置すると将来の判定変更で片方の直し忘れが起きる
- Low 相当

## 現状

```rust
// src/encoder.rs の validate_fps_numerator
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

// src/encoder.rs の validate_expected_frame_rate
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

`validate_positive_i32_field(field: &'static str, reason_overflow: &'static str, value: u32) -> Result<(), Error>` のような共通関数 1 本に統合する。エラー時の `reason: "must not be zero"` は両者共通なので関数内に閉じ込め、`field` / `reason_overflow` は呼び出し側で必ず指定する。

統合対象は `validate_fps_numerator` / `validate_expected_frame_rate` の 2 関数のみとする。`validate_config` 内の `max_key_frame_interval` / `max_frame_delay_count` のインライン検証も i32 上限チェックだが、ゼロ拒否を持たない（`NonZeroU32` がゼロを排除済み）ため、本統合のスコープには含めない。`validate_average_bitrate` は i64 上限、`validate_data_rate_limits` は上限個数 + bytes / window の検証であり、いずれも統合対象外。

## 関連 issue

- issue 0041（Error 型の String 化）は本 2 関数の `Error` 構築箇所を変更対象としており、統合後は共通関数内の 2 箇所が対象になる。どちらを先に実施しても成立する（0041 を先に実施した場合は、共通関数内の `Error` 構築に `.into()` の追加が必要になる）
- issue 0051（テストアサーション強化）は `encoder_rejects_fps_numerator_above_i32_max` の `reason` 固定を予定しており、本統合の回帰検出を補強する。こちらもどちらを先に実施しても成立する（統合後も `reason` 文字列は不変のため）

## 完了条件

- `validate_fps_numerator` / `validate_expected_frame_rate` が共通関数 1 本に統合され、旧 2 関数が削除されている（新関数は設計方針のシグネチャに従う）
- 呼び出し側 (`Encoder::validate_config`、`Encoder::validate_reconfigure_params`) が新関数を使う形に書き換わっている
- 既存のテスト (`encoder_rejects_zero_fps_numerator`、`encoder_rejects_fps_numerator_above_i32_max`、`reconfigure_rejects_zero_expected_frame_rate`、`reconfigure_rejects_expected_frame_rate_above_i32_max`) が同じエラーメッセージで通る
- `CHANGES.md` の `## develop` に `[UPDATE]` としてリファクタリングのエントリを追記する（公開 API の変更を伴わないため `### misc` サブセクション）
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

候補 (関数名は仮。`i32::MAX` 上限の検証であることが分かる名前を付けること):

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

## 解決方法

`src/encoder.rs` の `validate_fps_numerator` と `validate_expected_frame_rate` を削除し、設計方針どおりのシグネチャ `validate_positive_i32_field(field: &'static str, reason_overflow: &'static str, value: u32)` を持つ共通関数 1 本に統合した。

- ゼロ拒否時の `reason: "must not be zero"` は関数内に固定し、`field` / `reason_overflow` のみ呼び出し側から指定する
- `Encoder::validate_config` の `fps_numerator` 検証と `Encoder::validate_reconfigure_params` の `expected_frame_rate` 検証の 2 箇所を新関数の呼び出しに置き換えた
- エラーメッセージ (`field` / `reason`) は従来と完全に同一のため、既存テストは無変更で通る
- `CHANGES.md` の `## develop` に `[UPDATE]` として `### misc` サブセクションへ追記した
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test --workspace -- --test-threads=1` が通ることを確認した
