# `Encoder::reconfigure` を動的更新のみに絞り nvcodec-rs / amf-rs / vpl-rs と揃える

Created: 2026-05-11
Model: Opus 4.7 (claude-opus-4-7[1m])

## 概要

現在の `Encoder::reconfigure` は、設計途上で「動的プロパティ更新」と「セッション再作成」を一つの API で受け持つ構造になっている。
これを兄弟ライブラリ (`nvcodec-rs`, `amf-rs`, `vpl-rs`) および libwebrtc の VideoToolbox エンコーダ実装に合わせ、**動的更新のみ** を扱う API に絞り込む。
解像度・コーデック・フレームレートの変更は `Encoder` を作り直す運用に統一する。

## 背景

### 兄弟ライブラリの方針

| ライブラリ | 動的に変更可能な項目 | 解像度・コーデック変更 |
|---|---|---|
| nvcodec-rs | width / height / framerate / bitrate (`nvEncReconfigureEncoder`) | 不要 (基底 API が動的サポート) |
| amf-rs | framerate / bitrate / QP / GOP (`AMFPropertyStorage`) | 非サポート (Encoder を作り直す) |
| vpl-rs | framerate / bitrate (`MFXVideoENCODE_Reset`) | 非サポート (Encoder を作り直す) |

3 ライブラリとも `reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error>` のシグネチャで、`ReconfigureParams` を **所有権渡し** で受ける。
セッション再作成パスは持たない。

### libwebrtc の方針

`sdk/objc/components/video_codec/RTCVideoEncoderH264.mm` の実装を確認したところ、libwebrtc も同じ方針:

- 公開 API の動的変更は `setBitrate:framerate:` のみで、bitrate (kbps) と framerate (整数 1 つ) を受ける
- 内部で `kVTCompressionPropertyKey_AverageBitRate` / `_ExpectedFrameRate` / `_DataRateLimits` を `VTSessionSetProperty` で動的に上書きする
- width / height / codec は `startEncodeWithSettings:` で確定し、以降は `RTC_DCHECK_EQ(frame.width, _width)` で同一性を要求する
- セッション再作成は pixel format 変更時 (`resetCompressionSessionIfNeededWithFrame:`) や `kVTInvalidSessionErr` 等の障害復旧時の **内部リカバリ** に限定されており、外部 API としては提供していない
- framerate は `num/den` のペアではなく単一 `uint32_t`。VideoToolbox の `ExpectedFrameRate` が単一数値しか受け付けないため

### 現在の video-toolbox-rs の問題

直前の作業 (旧 issue 0040、廃棄済み) で「セッション再作成 + 動的更新」を `ReconfigureParams` 一つで受ける設計を導入したが、

- 兄弟ライブラリと API 形状が一致しない (引数の所有権受け渡し、フィールド構成)
- VideoToolbox の `VTCompressionSession` が width / height / codec / fps の動的変更を仕様上サポートしないため、`reconfigure` 内のセッション再作成パスは **「`Encoder::new` をユーザに代わって呼ぶラッパー」** に過ぎず、`Encoder` の状態 (`next_input_pts`、コールバック保持等) も同時に作り直すため透過的ではない
- libwebrtc 流に「セッション再作成は外部 API として持たない」方が、ユーザのコードベース全体での扱いが単純になる

## 対応方針

### `ReconfigureParams` の縮小

```rust
#[derive(Debug, Clone, Default)]
pub struct ReconfigureParams {
    /// kVTCompressionPropertyKey_AverageBitRate (bps)
    pub average_bitrate: Option<u64>,
    /// kVTCompressionPropertyKey_ExpectedFrameRate
    pub expected_frame_rate: Option<u32>,
    /// kVTCompressionPropertyKey_DataRateLimits
    pub data_rate_limits: Option<DataRateLimit>,
}
```

以下のフィールドは削除する。これらを変更したい場合は `Encoder` を新規に作り直す:

- `width`
- `height`
- `codec`
- `fps_numerator`
- `fps_denominator`

### `reconfigure` のシグネチャ変更

`&ReconfigureParams` → `ReconfigureParams` (所有権渡し) に変更し、3 兄弟ライブラリと揃える:

```rust
pub fn reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error>
```

### 内部実装の整理

- `recreate_session` を削除する
- `merge_config` を削除する (新しい `EncoderConfig` を構築する必要がなくなる)
- `apply_dynamic_properties` を `reconfigure` 本体に統合する (もしくは関数を残しつつ呼び出し階層を単純化する)
- `needs_recreate` / `has_dynamic_change` を削除する
- 全項目 `None` の場合は no-op で `Ok(())` を返す
- 動的更新の対象プロパティ (`average_bitrate` / `expected_frame_rate` / `data_rate_limits`) のうち、指定されたものを `VTSessionSetProperties` で一括上書きする (現行と同じ)

### `EncoderConfig` への反映

`reconfigure` が `VTSessionSetProperties` を成功させた後、現在保持している `EncoderConfig` の対応フィールドを更新する (現行の動的更新パスと同じ挙動)。

### フィールド名のリネームは行わない

`framerate_num` / `framerate_den` への変更は、VideoToolbox の `ExpectedFrameRate` が単一値しか受け付けない都合上、ペアにしてもミスリーディングになるため見送る。
libwebrtc も整数 1 つ (`uint32_t framerate`) で受けている。
`expected_frame_rate: Option<u32>` を維持する。

## 影響範囲

- `src/encoder.rs`: `ReconfigureParams` 構造体、`Encoder::reconfigure` 実装、関連ヘルパー (`merge_config` / `recreate_session` / `apply_dynamic_properties` / `needs_recreate` / `has_dynamic_change`) の削除・整理
- `src/lib.rs`: 公開シンボルは `ReconfigureParams` をそのまま維持 (フィールド構成のみ変わる)
- `examples/`: `reconfigure` 呼び出し箇所があれば追従
- `tests/test_encoder.rs`: 再作成パスのテストを削除し、動的更新のみのテストに置き換える
- `pbt/`: 該当する PBT があれば追従
- `CHANGES.md`: `## develop` セクションに `[CHANGE]` で記載する

## 検証

- `cargo build` / `cargo clippy --all-targets -- -D warnings` が通る
- `cargo test` で既存テストが通る
- `cargo llvm-cov` で `ReconfigureParams` 関連のカバレッジが既存比で落ちない (再作成パスの行が消えるため分母も変わる前提)

## 参考

- libwebrtc: `src/sdk/objc/components/video_codec/RTCVideoEncoderH264.mm`
  - `setBitrate:framerate:` (行 587-)
  - `setEncoderBitrateBps:frameRate:` (行 792-)
- nvcodec-rs: `src/encode.rs` の `Encoder::reconfigure` (`nvEncReconfigureEncoder`)
- amf-rs: `src/encode.rs` の `Encoder::reconfigure` (`AMFPropertyStorage` 経由)
- vpl-rs: `src/encode.rs` の `Encoder::reconfigure` (`MFXVideoENCODE_Reset`)
