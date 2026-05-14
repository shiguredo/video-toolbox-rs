# `add_common_properties` と `reconfigure` のプロパティ構築を共通化する

- Priority: Low
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-share-encoder-property-builders

## 目的

`Encoder` 内で `kVTCompressionPropertyKey_AverageBitRate` と `kVTCompressionPropertyKey_ExpectedFrameRate` を CFNumber 化して `properties` Vec に push する処理が、`add_common_properties` (`src/encoder.rs:446-548` 付近) と `reconfigure` (`src/encoder.rs:323-346` 付近) の 2 ヶ所に重複している。さらに `reconfigure` 側では `cf_objects: Vec<CfPtr<c_void>>` の使い方が `add_common_properties` の慣習と乖離している。

将来 `DataRateLimits` などの動的更新可能項目を増やすと、3 ヶ所目以降の追記が発生する前兆。

## 優先度根拠

- 機能面に影響のない構造改善であり、緊急度は低い
- 直近の reconfigure 拡張 (例えば `DataRateLimits` を `ReconfigureParams` に足す) で再度同じ重複を生み出す危険があるため、追加が出る前にやっておくと得策
- Low 相当

## 現状

### `add_common_properties` の該当抜粋

```rust
// ビットレート (指定時のみ設定)
if let Some(bitrate) = config.average_bitrate {
    let target_bitrate = cf_number_i64(bitrate as i64)?;
    properties.push((
        sys::kVTCompressionPropertyKey_AverageBitRate,
        target_bitrate.0,
    ));
    cf_objects.push(target_bitrate);
}

let fps = cf_number_i32(config.fps_numerator.div_ceil(config.fps_denominator) as i32)?;
properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, fps.0));
cf_objects.push(fps);
```

### `reconfigure` の該当抜粋

```rust
let mut properties: Vec<(sys::CFStringRef, *const c_void)> = Vec::new();
let mut cf_objects: Vec<CfPtr<c_void>> = Vec::new();

if let Some(bitrate) = params.average_bitrate {
    let value = cf_number_i64(bitrate as i64)?;
    properties.push((sys::kVTCompressionPropertyKey_AverageBitRate, value.0));
    cf_objects.push(value);
}
if let Some(fps) = params.expected_frame_rate {
    let value = cf_number_i32(fps as i32)?;
    properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, value.0));
    cf_objects.push(value);
}
```

CFNumber 型 (`i64` / `i32`) とキー名はそれぞれ完全に一致しており、構造を切り出せば 1 ヶ所で扱える。

注意: `add_common_properties` 側は分数 fps から整数 fps を `div_ceil` で導出するが、`reconfigure` 側は `ReconfigureParams::expected_frame_rate: u32` をそのまま使う。共通化する際は、整数 fps を受ける形のヘルパーにして、呼び出し側が事前に丸めるのが素直。

## 設計方針

以下のような単項目プロパティ追加ヘルパーを切り出す:

```rust
fn push_bitrate_property(
    properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
    cf_objects: &mut Vec<CfPtr<c_void>>,
    bitrate_bps: u64,
) -> Result<(), Error> { /* cf_number_i64 → push → cf_objects.push */ }

fn push_expected_frame_rate_property(
    properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
    cf_objects: &mut Vec<CfPtr<c_void>>,
    fps: u32,
) -> Result<(), Error> { /* cf_number_i32 → push → cf_objects.push */ }
```

これにより呼び出し側は以下のように圧縮される:

```rust
if let Some(bitrate) = params.average_bitrate {
    push_bitrate_property(&mut properties, &mut cf_objects, bitrate)?;
}
if let Some(fps) = params.expected_frame_rate {
    push_expected_frame_rate_property(&mut properties, &mut cf_objects, fps)?;
}
```

`reconfigure` 側で「`cf_objects` に push するパターン」が `add_common_properties` と統一されるため、コードを横に読む保守性が向上する。

## 完了条件

- bitrate / fps の CFNumber 構築 + push が 1 ヶ所のヘルパー関数に集約されている
- `Encoder::create_compression_session` 経由の初期化と `Encoder::reconfigure` の両方が同じヘルパーを使う
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る
- 既存テスト (`encode_h264_black`、`encode_h265_black`、`reconfigure_updates_config_on_success` 等) で挙動回帰がない

## 解決方法

- `Encoder<H>` の impl ブロック内に上記 2 つの `fn push_*_property` を追加 (関連関数として `Self::` 経由でアクセス可)
- `add_common_properties` 内の bitrate / fps ブロックをヘルパー呼び出しに置換
- `reconfigure` 内の bitrate / fps ブロックをヘルパー呼び出しに置換
- `unsafe` ブロックの範囲はヘルパー関数側に閉じ込め、呼び出し側を安全な呼び出しにする
