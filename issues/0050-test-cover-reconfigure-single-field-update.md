# `reconfigure` の片肺更新 (bitrate のみ / fps のみ) のテストを追加する

- Priority: Medium
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/add-reconfigure-single-field-update-tests

## 目的

`Encoder::reconfigure` の成功系 integration test (`tests/test_encoder.rs:420-436` の `reconfigure_updates_config_on_success`) は `average_bitrate` と `expected_frame_rate` を **両方同時に** 指定するケースのみを検証している。「`average_bitrate` のみ更新 (fps は不変)」「`expected_frame_rate` のみ更新 (bitrate は不変)」という単独更新パスが `tests/` 側に存在しない。

`src/encoder.rs:347-355` 付近の `if let Some(bitrate)` / `if let Some(fps)` 片肺更新パスを独立に保護する回帰テストが不足している。

## 優先度根拠

- 公開 API として「片肺更新は対側を不変に保つ」契約は `ReconfigureParams::default()` をベースにした利用者にとって基本中の基本
- 現状すり抜ける可能性は低いが、`reconfigure` の内部ロジックを変更したときに気付かないリスクを残している
- Medium

## 現状

- `tests/test_encoder.rs:420-436` の `reconfigure_updates_config_on_success` は両方更新のみを検証
- `tests/test_encoder.rs:438-452` の `reconfigure_is_noop_when_all_none` は両方 `None` のみを検証
- 片肺更新を行うテストは無い

実装側 (`src/encoder.rs:347-355` 付近):

```rust
if let Some(bitrate) = params.average_bitrate {
    self.config.average_bitrate = Some(bitrate);
}
if let Some(fps) = params.expected_frame_rate {
    self.config.fps_numerator = fps;
    self.config.fps_denominator = 1;
}
```

各 `if let Some` が独立しているので、片方だけが効いた状態の整合性 (対側が初期値のまま) を回帰検証する必要がある。

## 設計方針

`tests/test_encoder.rs` に以下 2 件の integration test を追加する。

- `reconfigure_updates_only_average_bitrate`:
  - `average_bitrate: Some(N)`、`expected_frame_rate: None`
  - 期待: `config().average_bitrate == Some(N)`、`config().fps_numerator` / `fps_denominator` が初期値のまま
- `reconfigure_updates_only_expected_frame_rate`:
  - `average_bitrate: None`、`expected_frame_rate: Some(N)`
  - 期待: `config().fps_numerator == N`、`config().fps_denominator == 1` (正規化される)、`config().average_bitrate` が初期値のまま

## 完了条件

- 上記 2 件のテストが `tests/test_encoder.rs` に追加されている
- それぞれ片肺更新後に「対側が不変」「更新側が反映」「fps 更新時の `fps_denominator = 1` 正規化」を assert している
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

```rust
#[test]
fn reconfigure_updates_only_average_bitrate() -> Result<(), Error> {
    let config = encoder_config(false);
    let initial_fps_num = config.fps_numerator;
    let initial_fps_den = config.fps_denominator;
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        expected_frame_rate: None,
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, initial_fps_num);
    assert_eq!(encoder.config().fps_denominator, initial_fps_den);
    Ok(())
}

#[test]
fn reconfigure_updates_only_expected_frame_rate() -> Result<(), Error> {
    let config = encoder_config(false);
    let initial_bitrate = config.average_bitrate;
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: Some(60),
    })?;
    assert_eq!(encoder.config().average_bitrate, initial_bitrate);
    assert_eq!(encoder.config().fps_numerator, 60);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}
```
