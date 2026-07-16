# `encoder_rejects_*` と `reconfigure_is_noop_*` のアサーションを強化する

- Priority: Low
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-tighten-encoder-test-assertions

## 目的

`tests/test_encoder.rs` のテストでスタイル不統一とアサーション強度不足が混在している。

1. **`encoder_rejects_*` 系の `reason` 文字列が固定されていない**
  - 新規追加された `reconfigure_rejects_*` 系 (`tests/test_encoder.rs:455-552`) は `reason` 文字列まで `matches!` で固定している
  - 既存の `encoder_rejects_fps_numerator_above_i32_max` (`tests/test_encoder.rs:233-246`)、`encoder_rejects_average_bitrate_above_i64_max` (`tests/test_encoder.rs:278-291`) は `field` までしか固定していない
  - `validate_*` ヘルパー化 (0046 で更に統合予定) により `reason` 文字列が独立した契約になったので、固定する方が回帰検出力が上がる
2. **`reconfigure_is_noop_when_all_none` (`tests/test_encoder.rs:438-452`) は config 不変のみ確認**
  - issue 0043 の当初設計では「no-op 後に `encode` が成功する」も期待値として書かれていたが、現状実装には反映されていない
  - VTSessionSetProperties が呼ばれていない確認は FFI 内部に踏み込めないので妥当として、最低限「その後の `encode` が成功する」ことを追加検証することでセッション破壊が起きていないことを保証できる

## 優先度根拠

- いずれも既存テストの強化であり、緊急度は低い
- 0046 (validate ヘルパー統合) と一緒にやると最小コストで揃えられる
- Low

## 現状

### 1. `reason` 文字列の固定状況

固定されている例 (新規追加):

```rust
// tests/test_encoder.rs:455-474
let err = encoder.reconfigure(ReconfigureParams { average_bitrate: Some(0), ... }).unwrap_err();
assert!(matches!(
    err,
    Error::InvalidConfig {
        field: "average_bitrate",
        reason: "must not be zero",
    }
));
```

固定されていない例 (既存):

```rust
// tests/test_encoder.rs:233-246
assert!(matches!(
    Encoder::new(c, ...),
    Err(Error::InvalidConfig { field: "fps_numerator", .. })
));
```

### 2. `reconfigure_is_noop_when_all_none` の検証

```rust
// tests/test_encoder.rs:438-452
encoder.reconfigure(ReconfigureParams::default())?;
assert_eq!(encoder.config().average_bitrate, before_bitrate);
assert_eq!(encoder.config().fps_numerator, before_fps_num);
assert_eq!(encoder.config().fps_denominator, before_fps_den);
```

no-op 後にエンコードが続行できるか (= セッションが壊れていないか) の確認が無い。

## 設計方針

### 1. `encoder_rejects_*` の `reason` 固定

以下のテストを `reason` 文字列固定型に書き換える:

- `encoder_rejects_zero_fps_denominator`: `reason: "must not be zero"`
- `encoder_rejects_fps_numerator_above_i32_max`: `reason: "must fit in i32 for CMTime timescale"`
- `encoder_rejects_average_bitrate_above_i64_max`: `reason: "must fit in i64 for CFNumber"`
- `encoder_rejects_zero_width` / `encoder_rejects_zero_height` 等: 該当する `reason` で固定

### 2. `reconfigure_is_noop_when_all_none` の検証強化

`reconfigure(ReconfigureParams::default())?` 直後に 1 フレーム encode を呼び、コールバック経由でエラーなく結果が返ることを確認する。

## 完了条件

- `encoder_rejects_*` 系テストが `reason` 文字列まで `matches!` 固定で書かれている
- `reconfigure_is_noop_when_all_none` が「no-op 後に encode 成功」も検証している
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

`reason` 固定の例:

```rust
#[test]
fn encoder_rejects_fps_numerator_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must fit in i32 for CMTime timescale",
        })
    ));
}
```

`reconfigure_is_noop` の強化例:

```rust
#[test]
fn reconfigure_is_noop_when_all_none() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |r| results.lock().expect("results mutex poisoned").push(r)
        }),
    )?;
    encoder.reconfigure(ReconfigureParams::default())?;
    let (y, u, v) = build_i420_black_frame();
    encoder.encode(&FrameData::I420 { y: &y, u: &u, v: &v }, &EncodeOptions::default(), 1)?;
    encoder.finish()?;
    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].is_ok(), "encode after no-op reconfigure must succeed");
    Ok(())
}
```

0046 (validate ヘルパー統合) と同じブランチでまとめて対応すると整合が取りやすい。
