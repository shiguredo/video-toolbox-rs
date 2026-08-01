# `add_common_properties` と `reconfigure` のプロパティ構築を共通化する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed: 2026-08-01
- Model: Opus 4.7
- Branch: feature/refactor-share-encoder-property-builders
- Polished: 2026-07-31

## 目的

`Encoder` 内で `kVTCompressionPropertyKey_AverageBitRate` と `kVTCompressionPropertyKey_ExpectedFrameRate` を CFNumber 化して `properties` Vec に push する処理が、`src/encoder.rs` の `add_common_properties` と `reconfigure` の 2 ヶ所に重複している。`kVTCompressionPropertyKey_DataRateLimits` は既に `push_data_rate_limits_property` として共通化ヘルパー化されており、bitrate / fps だけが 2 ヶ所重複のまま取り残されている。次の動的更新可能項目を追加するときに 3 ヶ所目の追記が発生する前兆である。

## 優先度根拠

- 機能面に影響のない構造改善であり、緊急度は低い
- 次の動的更新項目を追加する際に同じ重複を生み出す危険があるため、追加が出る前にやっておくと得策
- Low 相当

## 現状

### `add_common_properties` の該当抜粋（実ソースの順序どおり）

```rust
let fps = cf_number_i32(config.fps_numerator.div_ceil(config.fps_denominator) as i32)?;

// ビットレート (指定時のみ設定)
if let Some(bitrate) = config.average_bitrate {
    let target_bitrate = cf_number_i64(bitrate as i64)?;
    properties.push((
        sys::kVTCompressionPropertyKey_AverageBitRate,
        target_bitrate.0,
    ));
    cf_objects.push(target_bitrate);
}

properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, fps.0));
cf_objects.push(fps);
```

### `reconfigure` の該当抜粋

```rust
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

CFNumber 型 (`i64` / `i32`) とキー名はそれぞれ完全に一致しており、構造を切り出せば 1 ヶ所で扱える。bitrate ブロックは同一構造だが、fps は push の条件（`add_common_properties` は必ず設定、`reconfigure` は `Option` 指定時のみ）が異なる。

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
// reconfigure 側（fps は指定時のみ）
if let Some(bitrate) = params.average_bitrate {
    push_bitrate_property(&mut properties, &mut cf_objects, bitrate)?;
}
if let Some(fps) = params.expected_frame_rate {
    push_expected_frame_rate_property(&mut properties, &mut cf_objects, fps)?;
}
```

```rust
// add_common_properties 側（fps は必ず設定する）
let fps = config.fps_numerator.div_ceil(config.fps_denominator);
push_expected_frame_rate_property(&mut properties, &mut cf_objects, fps)?;
if let Some(bitrate) = config.average_bitrate {
    push_bitrate_property(&mut properties, &mut cf_objects, bitrate)?;
}
```

bitrate / fps の CFNumber 構築 + push の手続きが 1 箇所に集約され、コードを横に読む保守性が向上する。

## 完了条件

- bitrate / fps の CFNumber 構築 + push がヘルパー関数群（`push_bitrate_property` / `push_expected_frame_rate_property`）に集約されている
- `Encoder::create_compression_session` 経由の初期化と `Encoder::reconfigure` の両方が同じヘルパーを使う
- `CHANGES.md` の `## develop` に `[UPDATE]` としてリファクタリングのエントリを追記する（公開 API の変更を伴わないため `### misc` サブセクション）
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る
- 既存テスト (`encode_h264_black`、`encode_h265_black`、`reconfigure_updates_config_on_success` 等) で挙動回帰がない

## 解決方法

- `push_data_rate_limits_property` と同じモジュールレベルのフリー関数として、上記 2 つの `fn push_*_property` を追加する
- `add_common_properties` 内の bitrate / fps ブロックをヘルパー呼び出しに置換
- `reconfigure` 内の bitrate / fps ブロックをヘルパー呼び出しに置換
- ヘルパーは安全関数とし、`unsafe` はヘルパー内部のキー参照（`sys::kVTCompressionPropertyKey_*`）に閉じ込める（`push_data_rate_limits_property` と同じ構成。呼び出し側の `unsafe` ブロックは他のプロパティ設定のため残る）

## 解決方法

`src/encoder.rs` に `push_bitrate_property` / `push_expected_frame_rate_property` を追加し、`add_common_properties` と `reconfigure` の bitrate / fps ブロックをヘルパー呼び出しに置換した。

- ヘルパーは `push_data_rate_limits_property` と同じ構成のモジュールレベルフリー関数で、`unsafe` はヘルパー内部のキー参照 (push 部分) に閉じ込めた
- `add_common_properties` 側は `div_ceil` で整数 fps を導出してからヘルパーへ渡し、`reconfigure` 側は `ReconfigureParams::expected_frame_rate` をそのまま渡す (設計方針どおり)
- ヘルパーの doc コメントに `cf_objects` への所有移行の必要性 (use-after-free 防止) と検証済みキャストの前提を明記した
- 挙動は従来と同一で、既存テストは無変更で通る
- `CHANGES.md` の `## develop` に `[UPDATE]` として `### misc` サブセクションへ追記した
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test --workspace -- --test-threads=1` が通ることを確認した
