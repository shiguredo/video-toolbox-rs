# `Encoder::config` getter の単体テストを追加する

- Priority: Medium
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/add-encoder-config-getter-test

## 目的

`Encoder::config(&self) -> &EncoderConfig` getter は本ブランチで新規追加された公開 API (`src/encoder.rs:272-282`) だが、`tests/test_encoder.rs` 内で独立に検証されているテストが存在しない。`reconfigure_*` 系テストの過程で間接的に呼ばれているのみで、「`Encoder::new` 直後に `encoder.config()` が初期 `EncoderConfig` を変更なしで返す」という基本契約の回帰テストが無い。

## 優先度根拠

- 公開 API として追加されたゲッターは独立にテストすべき
- 将来 `Encoder` 内部で `config` フィールドの保持方法を変更したとき、間接テスト経由では問題が表面化しにくい
- 致命的ではないが品質保証のため Medium

## 現状

- `src/encoder.rs:272-282`: `pub fn config(&self) -> &EncoderConfig` 定義
- `tests/test_encoder.rs`: `encoder.config()` は `reconfigure_updates_config_on_success` (`tests/test_encoder.rs:420-436`) と `reconfigure_is_noop_when_all_none` (`tests/test_encoder.rs:438-452`) でしか呼ばれない
- 「`Encoder::new` 直後の config が引数と同等」「`config()` が `&` 参照を返す」基本検証は無い

## 設計方針

`encoder_config_returns_initial_value` のような名前で integration test を追加する。`Encoder::new` 直後に `encoder.config()` を呼び、入力した `EncoderConfig` の主要フィールド (`width`、`height`、`fps_numerator`、`fps_denominator`、`pixel_format`、`average_bitrate`、`codec` のバリアント、`real_time`、`allow_frame_reordering` 等) を網羅的に比較する。

`EncoderConfig` 構造体には `Eq` / `PartialEq` 実装が無いため `assert_eq!` を直接使えない。各フィールド単位で `assert_eq!` する形になる。または `EncoderConfig` 自体に `#[derive(PartialEq, Eq)]` を追加する判断は別 issue 化 (現状で `Eq` 派生していないのは `Option<Duration>` などの理由かもしれず要調査)。

## 完了条件

- `tests/test_encoder.rs` に `encoder_config_returns_initial_value` (もしくは同等のテスト名) が追加されている
- テストが `Encoder::new` で渡した `EncoderConfig` の主要フィールドを `encoder.config()` 経由で取り出して assert している
- `cargo test --test test_encoder` でテストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

```rust
#[test]
fn encoder_config_returns_initial_value() -> Result<(), Error> {
    let config = encoder_config(false);
    let encoder = Encoder::new(
        config.clone(),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
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
    // codec バリアントは matches! で確認する
    assert!(matches!(got.codec, CodecConfig::H264(_)));
    Ok(())
}
```

`EncoderConfig` が `Clone` 派生済みであることを前提にする。`Clone` 派生が無ければ別途追加する。
