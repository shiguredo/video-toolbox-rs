# `encoder_rejects_*` と `reconfigure_is_noop_*` のアサーションを強化する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed: 2026-08-06
- Model: Opus 4.7
- Branch: feature/update-tighten-encoder-test-assertions
- Polished: 2026-07-31

## 目的

`tests/test_encoder.rs` のテストでスタイル不統一とアサーション強度不足が混在している。

1. **`encoder_rejects_*` 系の一部の `reason` 文字列が固定されていない**
  - 新規追加された `reconfigure_rejects_*` 系（`data_rate_limits` 系まで含む）は `reason` 文字列まで `matches!` で固定している
  - 既存の `encoder_rejects_fps_numerator_above_i32_max`、`encoder_rejects_average_bitrate_above_i64_max` 等は `field` までしか固定していない
  - `Error::InvalidConfig.reason` は公開 API として観測可能な契約であり、固定する方が回帰検出力が上がる
2. **`reconfigure_is_noop_when_all_none` は config 不変のみ確認**
  - issue 0043 の当初設計では「no-op 後に `encode` が成功する」も期待値として書かれていたが、現状実装には反映されていない
  - VTSessionSetProperties が呼ばれていない確認は FFI 内部に踏み込めないので妥当として、最低限「その後の `encode` が成功する」ことを追加検証することでセッション破壊が起きていないことを保証できる

## 優先度根拠

- いずれも既存テストの強化であり、緊急度は低い
- Low

## 現状

### 1. `reason` 文字列の固定状況

固定されている例（新規追加の `reconfigure_rejects_zero_bitrate`。`reconfigure_err` ヘルパーで検証する形式）:

```rust
#[test]
fn reconfigure_rejects_zero_bitrate() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            average_bitrate: Some(0),
            ..Default::default()
        }),
        Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must not be zero",
        }
    ));
}
```

固定されていない例（既存の `encoder_rejects_fps_numerator_above_i32_max`。`field` のみ固定し `reason` を `..` で無視する）:

```rust
#[test]
fn encoder_rejects_fps_numerator_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            ..
        })
    ));
}
```

### 2. `reconfigure_is_noop_when_all_none` の検証

```rust
encoder.reconfigure(ReconfigureParams::default())?;
assert_eq!(encoder.config().average_bitrate, before_bitrate);
assert_eq!(encoder.config().fps_numerator, before_fps_num);
assert_eq!(encoder.config().fps_denominator, before_fps_den);
```

no-op 後にエンコードが続行できるか (= セッションが壊れていないか) の確認が無い。

## 設計方針

### 1. `encoder_rejects_*` の `reason` 固定

以下のテストを `reason` 文字列固定型に書き換える:

- `encoder_rejects_zero_width`: `reason: "must not be zero"`
- `encoder_rejects_zero_height`: `reason: "must not be zero"`
- `encoder_rejects_fps_numerator_above_i32_max`: `reason: "must fit in i32 for CMTime timescale"`
- `encoder_rejects_width_above_i32_max`: `reason: "must fit in i32 for Video Toolbox dimensions"`
- `encoder_rejects_height_above_i32_max`: `reason: "must fit in i32 for Video Toolbox dimensions"`
- `encoder_rejects_average_bitrate_above_i64_max`: `reason: "must fit in i64 for CFNumber"`
- `encoder_rejects_zero_fps_denominator`: `reason: "must not be zero"`

なお `encoder_rejects_zero_fps_numerator` は既に `reason` 固定済みのため対象外。

### 2. `reconfigure_is_noop_when_all_none` の検証強化

`reconfigure(ReconfigureParams::default())?` 直後に 1 フレーム encode を呼び、コールバック経由でエラーなく結果が返ることを確認する。既存の config 不変のアサーションは残す。

## 関連 issue

- issue 0041（Error 型の String 化）は `Error::InvalidConfig` の `field` / `reason` を `String` 化するため、本 issue の `matches!` リテラルパターンは 0041 適用後はコンパイルエラーになり match guard への変更が必要になる。どちらを先に実施しても成立する（0041 を先に実施する場合は、本 issue の修正を match guard 形式で書く）
- issue 0046（fps バリデータ統合）は `encoder_rejects_fps_numerator_above_i32_max` の `reason` 固定を本 issue のスコープと整理済み。どちらを先に実施しても成立する（統合後も `reason` 文字列は不変のため）
- issue 0043（reconfigure の動的更新化。`issues/closed/0043-encoder-reconfigure-dynamic-only.md`）: 本 issue の 2 項目目が参照する「no-op 後に `encode` が成功する」という当初設計の出典

## 完了条件

- `encoder_rejects_*` 系テストが `reason` 文字列まで `matches!` 固定で書かれている（0041 を先に実施した場合は match guard で比較）
- `reconfigure_is_noop_when_all_none` が「no-op 後に encode 成功」も検証しつつ、既存の config 不変の検証を維持している
- `CHANGES.md` の `## develop` に `[UPDATE]` としてテスト強化のエントリを追記する（`### misc` サブセクション）
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

`reason` 固定の例:

```rust
#[test]
fn encoder_rejects_fps_numerator_above_i32_max() {
    // i32 の上限を超える fps_numerator は CMTime timescale 用の理由で拒否されること
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must fit in i32 for CMTime timescale",
        })
    ));
}
```

0041（Error の String 化）を先に実施した場合は、上記を match guard 形式で書く:

```rust
Err(Error::InvalidConfig { field, reason })
    if field == "fps_numerator" && reason == "must fit in i32 for CMTime timescale"
```

`reconfigure_is_noop` の強化例:

```rust
#[test]
fn reconfigure_is_noop_when_all_none() -> Result<(), Error> {
    // 全項目 None の reconfigure は no-op であり、設定を変えずセッションも壊さないことを確認する
    let config = encoder_config(false);
    let before_bitrate = config.average_bitrate;
    let before_fps_num = config.fps_numerator;
    let before_fps_den = config.fps_denominator;
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;
    encoder.reconfigure(ReconfigureParams::default())?;
    assert_eq!(encoder.config().average_bitrate, before_bitrate);
    assert_eq!(encoder.config().fps_numerator, before_fps_num);
    assert_eq!(encoder.config().fps_denominator, before_fps_den);
    let (y, u, v) = build_i420_black_frame();
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        1,
    )?;
    encoder.finish()?;
    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].is_ok(), "no-op の reconfigure 後の encode は成功すること");
    Ok(())
}
```

## 解決方法

`tests/test_encoder.rs` のテストアサーションを強化した。

1. `encoder_rejects_*` 系 7 テスト (`encoder_rejects_zero_width` / `encoder_rejects_zero_height` / `encoder_rejects_fps_numerator_above_i32_max` / `encoder_rejects_width_above_i32_max` / `encoder_rejects_height_above_i32_max` / `encoder_rejects_average_bitrate_above_i64_max` / `encoder_rejects_zero_fps_denominator`) の `Error::InvalidConfig` 検証を `reason` 文字列まで固定する match guard 形式に変更した。`reason` は公開 API として観測可能な契約であり、回帰検出力が向上する
2. `reconfigure_is_noop_when_all_none` に no-op 後の `encode` 成功検証を追加した。1 フレームを encode し、コールバックで `user_data` と実データ (非空) が返ることを確認することで、no-op の reconfigure がセッションを壊していないことを検証する。既存の config 不変の検証は維持した

`CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記した。`cargo test --workspace` は全 46 テストがパスする。
