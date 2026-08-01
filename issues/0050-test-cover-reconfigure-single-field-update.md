# `reconfigure` の片肺更新 (bitrate のみ / fps のみ) のテストを追加する

- Priority: Medium
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/add-reconfigure-single-field-update-tests
- Polished: 2026-07-31

## 目的

`Encoder::reconfigure` の成功系 integration test（`tests/test_encoder.rs` の `reconfigure_updates_config_on_success`）は `average_bitrate` と `expected_frame_rate` を **両方同時に** 指定するケースのみを検証している。「`average_bitrate` のみ更新 (fps は不変)」「`expected_frame_rate` のみ更新 (bitrate は不変)」という単独更新パスの config 整合性が `tests/` 側に存在しない。

## 優先度根拠

- 公開 API として「片肺更新は対側を不変に保つ」契約は `ReconfigureParams::default()` をベースにした利用者にとって基本中の基本
- 現状すり抜ける可能性は低いが、`reconfigure` の内部ロジックを変更したときに気付かないリスクを残している
- Medium

## 現状

- `tests/test_encoder.rs` の `reconfigure_updates_config_on_success` は両方更新のみを検証
- `tests/test_encoder.rs` の `reconfigure_is_noop_when_all_none` は両方 `None` のみを検証
- 片肺更新を行う integration test は無い（`data_rate_limits` 単独更新については `reconfigure_updates_data_rate_limits` が別途カバーしているが、bitrate / fps の単独更新は未カバー）。なお `src/encoder.rs` の `#[cfg(test)] mod tests` 内のテスト（例: `reconfigure_preserves_next_input_pts_when_only_bitrate_changes` / `reconfigure_rescales_next_input_pts_on_frame_rate_change`）は片肺更新パスを実行しているが、`next_input_pts` のみを検証しており、config 値の対側不変・正規化は検証していない

実装側 (`src/encoder.rs` の `Encoder::reconfigure` 内):

```rust
if let Some(bitrate) = params.average_bitrate {
    self.config.average_bitrate = Some(bitrate);
}
if let Some(fps) = params.expected_frame_rate {
    // ExpectedFrameRate は単一整数のため分母を 1 に正規化する。
    self.config.fps_numerator = fps;
    self.config.fps_denominator = 1;
}
```

各 `if let Some` が独立しているので、片方だけが効いた状態の整合性 (対側が初期値のまま) を回帰検証する必要がある。

## 設計方針

`tests/test_encoder.rs` に以下 2 件の integration test を追加する。両テストとも初期 fps は分数 (30_000/1_001) にする。テスト 1 は「fps が既定値 (1/1) に巻き戻る・分母が 1 に正規化される」回帰を、テスト 2 は「`fps_denominator = 1` への正規化を忘れる」回帰を検出できる（初期 fps を 1/1 のままにすると、これらの assert が常に真になり検出力が無くなる）。

- `reconfigure_updates_only_average_bitrate`:
  - `average_bitrate: Some(N)`、`expected_frame_rate: None`。初期 fps は 30_000/1_001 にしておく
  - 期待: `config().average_bitrate == Some(N)`、`config().fps_numerator` / `fps_denominator` が初期値 (30_000/1_001) のまま
- `reconfigure_updates_only_expected_frame_rate`:
  - `average_bitrate: None`、`expected_frame_rate: Some(N)`。初期 fps は 30_000/1_001 にしておく
  - 期待: `config().fps_numerator == N`、`config().fps_denominator == 1` (正規化される)、`config().average_bitrate` が初期値のまま

## 完了条件

- 上記 2 件のテストが `tests/test_encoder.rs` に追加されている
- それぞれ片肺更新後に「対側が不変」「更新側が反映」「fps 更新時の `fps_denominator = 1` 正規化」を assert している
- 初期 fps は既定値 (1/1) と区別できる分数 (30_000/1_001) を使い、assert の検出力を確保している
- `CHANGES.md` の `## develop` にテスト追加のエントリを追記する（`### misc` サブセクション）
- `cargo test --test test_encoder` で全テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

```rust
#[test]
fn reconfigure_updates_only_average_bitrate() -> Result<(), Error> {
    // bitrate のみ更新で fps (30_000/1_001) が初期値のまま保たれることを確認する
    let mut config = encoder_config(false);
    config.fps_numerator = 30_000;
    config.fps_denominator = 1_001;
    let initial_fps_num = config.fps_numerator;
    let initial_fps_den = config.fps_denominator;
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, initial_fps_num);
    assert_eq!(encoder.config().fps_denominator, initial_fps_den);
    Ok(())
}

#[test]
fn reconfigure_updates_only_expected_frame_rate() -> Result<(), Error> {
    // fps のみ更新で bitrate が初期値のまま保たれ、分母が 1 に正規化されることを確認する
    let mut config = encoder_config(false);
    config.fps_numerator = 30_000;
    config.fps_denominator = 1_001;
    let initial_bitrate = config.average_bitrate;
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        expected_frame_rate: Some(60),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().average_bitrate, initial_bitrate);
    assert_eq!(encoder.config().fps_numerator, 60);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}
```
