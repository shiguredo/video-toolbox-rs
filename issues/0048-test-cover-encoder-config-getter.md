# `Encoder::config` getter の単体テストを追加する

- Priority: Medium
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/add-encoder-config-getter-test
- Polished: 2026-07-31

## 目的

`Encoder::config(&self) -> &EncoderConfig` getter は公開 API として追加済み（`CHANGES.md` の develop に `[ADD]` 記載）だが、`tests/test_encoder.rs` 内で独立に検証されているテストが存在しない。`reconfigure_*` 系テストや `new_normalizes_empty_data_rate_limits_to_none` などの過程で個別フィールドが間接的に確認されているのみで、「`Encoder::new` 直後に `encoder.config()` が初期 `EncoderConfig` を変更なしで返す（`data_rate_limits` の正規化を除く）」という基本契約の回帰テストが無い。なお、`Encoder::config` 以外の未カバー公開 API のテストは issue 0071（未カバーの公開 API テスト）で扱うため、本 issue の対象との重複はない。

## 優先度根拠

- 公開 API として追加されたゲッターは独立にテストすべき
- 将来 `Encoder` 内部で `config` フィールドの保持方法を変更したとき、間接テスト経由では問題が表面化しにくい
- 致命的ではないが品質保証のため Medium

## 現状

- `src/encoder.rs` の `Encoder::config`: `pub fn config(&self) -> &EncoderConfig` 定義（rustdoc 含む）
- `tests/test_encoder.rs`: `encoder.config()` は `reconfigure_updates_config_on_success`、`reconfigure_is_noop_when_all_none`、`reconfigure_updates_data_rate_limits`、`new_normalizes_empty_data_rate_limits_to_none` でのみ呼ばれる（いずれも個別フィールドの検証にとどまる。`src/encoder.rs` の `#[cfg(test)] mod tests` 内の呼び出しを除く）
- 「`Encoder::new` 直後の config が引数と同等」「`config()` が `&` 参照を返す」全フィールド網羅の基本検証は無い

## 設計方針

`encoder_config_returns_initial_value` のような名前で単体テストを追加する。`Encoder::new` 直後に `encoder.config()` を呼び、入力した `EncoderConfig` の全フィールドを比較する。

`EncoderConfig` 構造体には `Eq` / `PartialEq` 実装が無いため `assert_eq!` を直接使えない。各フィールド単位で `assert_eq!` する形になる（`codec` を除く全フィールドが `PartialEq` を持つことを確認済み。`codec` はバリアント分解して中身の `H264Profile` / `H264EntropyMode` を比較する）。なお `EncoderConfig` への `#[derive(PartialEq, Eq)]` 追加は本 issue のスコープ外とする（`CodecConfig` / `H264EncoderConfig` / `HevcEncoderConfig` への `PartialEq` 追加を伴う設計判断のため。必要になれば別途検討する）。

入力には既定値と区別できる値を使う。全フィールドを既定値で埋めると、実装が「ハードコードされた既定値」を返す回帰（config の保持漏れ）を検出できないため。特に bool / Option フィールドは既定値（false / None）に化ける回帰を検出できるよう、すべて既定値と異なる値にする。なお `data_rate_limits` は `Some(空 Vec)` のみ `Encoder::new` で `None` に正規化されるため、比較には空でない `Some` を使う。

## 完了条件

- `tests/test_encoder.rs` に `encoder_config_returns_initial_value` (もしくは同等のテスト名) が追加されている
- テストが `Encoder::new` で渡した `EncoderConfig` の全フィールドを `encoder.config()` 経由で取り出して assert している
- `cargo test --test test_encoder` でテストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

```rust
#[test]
fn encoder_config_returns_initial_value() -> Result<(), Error> {
    let mut config = encoder_config(false);
    // 既定値と区別できる値にする (ハードコードされた既定値を返す回帰を検出するため)
    config.fps_numerator = 30;
    config.real_time = true;
    config.prioritize_encoding_speed_over_quality = true;
    config.maximize_power_efficiency = true;
    config.allow_frame_reordering = true;
    config.max_key_frame_interval = std::num::NonZeroU32::new(60);
    config.max_key_frame_interval_duration = Some(Duration::from_secs(2));
    config.max_frame_delay_count = std::num::NonZeroU32::new(2);
    config.data_rate_limits = Some(vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    }]);
    let encoder = Encoder::new(config.clone(), noop_encode_handler())?;
    let got = encoder.config();
    assert_eq!(got.width, config.width);
    assert_eq!(got.height, config.height);
    assert_eq!(got.fps_numerator, config.fps_numerator);
    assert_eq!(got.fps_denominator, config.fps_denominator);
    assert_eq!(got.average_bitrate, config.average_bitrate);
    assert_eq!(got.pixel_format, config.pixel_format);
    assert_eq!(got.real_time, config.real_time);
    assert_eq!(got.allow_frame_reordering, config.allow_frame_reordering);
    assert_eq!(got.allow_temporal_compression, config.allow_temporal_compression);
    assert_eq!(
        got.prioritize_encoding_speed_over_quality,
        config.prioritize_encoding_speed_over_quality
    );
    assert_eq!(got.maximize_power_efficiency, config.maximize_power_efficiency);
    assert_eq!(got.max_key_frame_interval, config.max_key_frame_interval);
    assert_eq!(
        got.max_key_frame_interval_duration,
        config.max_key_frame_interval_duration
    );
    assert_eq!(got.max_frame_delay_count, config.max_frame_delay_count);
    assert_eq!(got.data_rate_limits, config.data_rate_limits);
    // codec はバリアントと中身を確認する
    match (&got.codec, &config.codec) {
        (CodecConfig::H264(a), CodecConfig::H264(b)) => {
            assert_eq!(a.profile, b.profile);
            assert_eq!(a.entropy_mode, b.entropy_mode);
        }
        _ => panic!("codec variant mismatch"),
    }
    Ok(())
}
```

`EncoderConfig` は `Clone` 派生済みであることを前提にする（`src/encoder.rs` の `#[derive(Debug, Clone)]`）。
