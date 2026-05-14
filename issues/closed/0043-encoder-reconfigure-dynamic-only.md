# `Encoder::reconfigure` を動的更新のみに絞り兄弟ライブラリと揃える

Created: 2026-05-11
Completed: 2026-05-11
Model: Opus 4.7

## 概要

現状の `Encoder::reconfigure` は `EncoderConfig` を所有権で受け取り、未出力フレームをフラッシュした上で `VTCompressionSession` を完全に作り直す実装になっている (`src/encoder.rs:195-215`)。
これを廃止し、新たに `ReconfigureParams` を導入して **動的プロパティ更新のみ** を行う API に置き換える。
シグネチャを兄弟ライブラリ (`nvcodec-rs`, `amf-rs`, `vpl-rs`) および libwebrtc の VideoToolbox エンコーダ実装と揃え、解像度・コーデック・ピクセルフォーマットの変更は `Encoder` 再生成で対応する運用に統一する。

## 現状

- `Encoder::reconfigure(&mut self, config: EncoderConfig) -> Result<(), Error>` が公開されている (`src/encoder.rs:195`)
- 実装は `self.finish()?` で未出力フレームをフラッシュ → `create_compression_session` で新セッション作成 → 旧セッション破棄 → `self.config` 差し替え → `self.next_input_pts = 0`
- 動的プロパティ更新パスは存在しない (`VTSessionSetProperties` を `reconfigure` から呼ぶコードは無い)
- `ReconfigureParams` 構造体、`DataRateLimit` 型、関連ヘルパーはいずれも存在しない
- `tests/test_encoder.rs` に `reconfigure` を呼ぶテストは 0 件
- `examples/raden_to_mp4.rs` に `reconfigure` 呼び出しは無い

このセッション再作成型 `reconfigure` は `closed/0004-encoder-dynamic-resolution-change.md` で導入されたものを、本 issue で置き換える。

## 背景

### 兄弟ライブラリの方針

3 ライブラリとも `reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error>` のシグネチャを採り、`ReconfigureParams` は **所有権渡し** で受ける。セッション再作成パスは公開 API として持たない (解像度・コーデック変更は `Encoder` 作り直しに統一)。

| ライブラリ | `ReconfigureParams` の主な項目 | 動的更新の基底 API |
|---|---|---|
| nvcodec-rs | `width` / `height` / `framerate_num` / `framerate_den` / `average_bitrate` / `max_bitrate` | `nvEncReconfigureEncoder` |
| amf-rs | `framerate_num` / `framerate_den` / `target_kbps` / `max_kbps` / QP 各種 / `gop_pic_size` | `AMFPropertyStorage` |
| vpl-rs | `target_kbps` / `max_kbps` / `framerate_num` / `framerate_den` | `MFXVideoENCODE_Reset` |

フィールド構成・単位 (bps / kbps) ・型は各バックエンドの API 都合で揃っていない。本 issue では「シグネチャ形状と所有権渡しの方針」のみを揃え、フィールド構成は VideoToolbox の動的更新可能プロパティに従う。

### libwebrtc の方針

`sdk/objc/components/video_codec/RTCVideoEncoderH264.mm` を参照する (Chromium に取り込まれている WebRTC ソース。行番号はチェックアウトで変動するため省略)。

- 公開 API の動的変更は `setBitrate:framerate:` のみで、bitrate (kbps) と framerate (`uint32_t`) を受ける
- 内部で `kVTCompressionPropertyKey_AverageBitRate` / `_ExpectedFrameRate` / `_DataRateLimits` を `VTSessionSetProperty` で上書きする
- width / height / codec は `startEncodeWithSettings:` で確定し、以降は `frame.width == _width` を要求する
- セッション再作成は pixel format 変更時や `kVTInvalidSessionErr` 等の障害復旧時の **内部リカバリ** に限定され、外部 API としては提供しない
- framerate は単一 `uint32_t`。VideoToolbox の `ExpectedFrameRate` が単一数値しか受け付けないため

## 対応方針

### `ReconfigureParams` の新規追加

`src/encoder.rs` に以下を新規追加し、`src/lib.rs` から再エクスポートする。

```rust
/// `Encoder::reconfigure` で動的に更新可能なエンコードパラメータ
///
/// `None` のフィールドは現在値を維持する。全項目 `None` の場合は no-op。
#[derive(Debug, Clone, Default)]
pub struct ReconfigureParams {
    /// kVTCompressionPropertyKey_AverageBitRate (bps 単位)
    pub average_bitrate: Option<u64>,

    /// kVTCompressionPropertyKey_ExpectedFrameRate (整数 fps)
    pub expected_frame_rate: Option<u32>,
}
```

- `width` / `height` / `codec` / `pixel_format` / `fps_*` および `EncoderConfig` のその他フィールドは含めない。これらを変更する場合はユーザ側で `Encoder` を作り直す
- `kVTCompressionPropertyKey_DataRateLimits` は本 issue ではスコープ外とし、必要になった時点で別 issue で追加する (新規型 `DataRateLimit` の定義と CFArray ヘルパー導入が必要なため)
- `max_key_frame_interval` 等の他の動的変更可能プロパティも本 issue ではスコープ外。最初は libwebrtc の `setBitrate:framerate:` と等価な 2 項目に限定する

### `reconfigure` のシグネチャと実装

```rust
pub fn reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error>
```

実装方針:

- 全項目 `None` の場合は `VTSessionSetProperties` を呼ばずに `Ok(())` を返す
- `params` を検証する (検証関数を新設):
  - `average_bitrate` が `Some` のとき `i64::MAX` 以下 (CFNumber `i64` の制約)
  - `expected_frame_rate` が `Some` のとき `0` でなく `i32::MAX` 以下 (CFNumber `i32` の制約および 0 fps 拒否)
- `Some` の項目だけを `cf_dictionary` に詰め、`VTSessionSetProperties(self.session, dict)` を 1 回呼んで一括反映する (CFNumber 生成は既存の `cf_number_i64` / `cf_number_i32` を利用し、生存期間は既存の `CfPtr` ガードで管理)
- `VTSessionSetProperties` が成功してから、`self.config` の対応フィールドを更新する。失敗時は `self.config` を変更せず、セッションも生かしたままエラーをそのまま返す:
  - `average_bitrate` → `self.config.average_bitrate = Some(value)`
  - `expected_frame_rate` → `self.config.fps_numerator = value`, `self.config.fps_denominator = 1` (PTS 計算の `CMTimeMake` timescale が `fps_numerator` を参照するため、単一整数を直接マッピングする)
    - 注意: `Encoder::new` で `fps_numerator/fps_denominator` を分数 (例: `30000/1001` で 29.97 fps) で指定していた場合でも、`reconfigure` を呼んだ瞬間に `fps_denominator` は `1` に書き換わる。これは VideoToolbox の `ExpectedFrameRate` が単一整数しか受け付けない仕様および libwebrtc 準拠の挙動。分数フレームレートを維持したい場合は `reconfigure` を使わず `Encoder` を作り直す
- `self.finish()` は呼ばない (動的更新ではフレームのフラッシュ不要)
- `self.next_input_pts` をリセットしない (セッションが継続するため PTS 列も継続させる)
- `self.callback` は維持する

### 旧 `reconfigure(EncoderConfig)` の扱い

公開 API として削除する。これは `closed/0004-encoder-dynamic-resolution-change.md` で導入されたが、VideoToolbox が解像度・コーデックの動的変更を仕様上サポートせず、再作成パスは「`Encoder::new` のラッパー」に過ぎないため、`Encoder` 再生成で代替できる。
内部のセッション作成ロジック (`create_compression_session`) は `Encoder::new` から引き続き利用する。

### `Encoder::config()` ゲッターの追加

`reconfigure` が `self.config` の対応フィールドを更新した結果を呼び出し側および単体テストから確認するため、現在の `EncoderConfig` を不変参照で返すゲッターを追加する。

```rust
pub fn config(&self) -> &EncoderConfig
```

ユーザ視点でも「現在エンコーダーが保持している設定」を取り出す API として自然な追加であり、後方互換性のある [ADD] となる。

## 影響範囲

- `src/encoder.rs`: `ReconfigureParams` 構造体追加、`reconfigure` 本体を `VTSessionSetProperties` ベースに置き換え、検証関数追加、`Encoder::config()` ゲッター追加
- `src/lib.rs`: `ReconfigureParams` を `pub use` で公開
- `tests/test_encoder.rs`: 以下のテストを **新規追加** する (現状 `reconfigure` テストは 0 件):
  - `reconfigure_no_op_when_all_none`: 全項目 `None` で `Ok(())`、その後 `encode` が成功する
  - `reconfigure_updates_average_bitrate`: `average_bitrate` 更新後に `Encoder` 内部の `config.average_bitrate` が反映される
  - `reconfigure_updates_expected_frame_rate`: `expected_frame_rate` 更新後に `config.fps_numerator` が値に、`fps_denominator` が `1` になる
  - `reconfigure_rejects_zero_expected_frame_rate`: `Some(0)` で `Error::InvalidConfig`
  - `reconfigure_rejects_expected_frame_rate_above_i32_max`: `Some(i32::MAX as u32 + 1)` で `Error::InvalidConfig`
  - `reconfigure_rejects_average_bitrate_above_i64_max`: `Some(i64::MAX as u64 + 1)` で `Error::InvalidConfig`
  - `reconfigure_preserves_next_input_pts`: `encode` 2 回 → `reconfigure` → `encode` 2 回でコールバックの順序と PTS 連続性を確認
- `examples/raden_to_mp4.rs`: 現状 `reconfigure` 呼び出しなし。修正不要 (確認済み)
- `pbt/`: 本リポジトリに `pbt/` ディレクトリは存在しない。本 issue では追加しない
- `README.md`: 「動的フォーマット変更 → エンコーダー」節と「まとめ」表 (`reconfigure()` 行) の説明・サンプルコードを `ReconfigureParams` ベースに書き換える。`closed/0004` 由来の「常にセッション破棄 + 再作成」記述は削除し、「動的プロパティ更新のみ。解像度・コーデック変更は `Encoder` 再生成」に直す
- `CHANGES.md`: `## develop` に `[CHANGE]` を追記する。例:

  ```
  - [ADD] `ReconfigureParams` 構造体を追加する
    - @voluntas
  - [ADD] `Encoder::config()` ゲッターを追加する
    - @voluntas
  - [CHANGE] `Encoder::reconfigure` を動的プロパティ更新専用 API に変更する
    - 引数を `EncoderConfig` から `ReconfigureParams` (所有権渡し) に変更する
    - 動的に変更可能な項目を `average_bitrate` / `expected_frame_rate` に限定する
    - 解像度・コーデック・ピクセルフォーマットの変更は `Encoder` 再生成で対応する
    - @voluntas
  ```

## 検証

- `cargo build` / `cargo clippy --all-targets -- -D warnings` が通る
- `cargo test -p shiguredo_video_toolbox --test test_encoder` で新規テストが通る
- `cargo llvm-cov --no-report -p shiguredo_video_toolbox --lib -- encoder` と `--test test_encoder` をマージして `ReconfigureParams` 関連のカバレッジを確認する

## 参考

- libwebrtc: `src/sdk/objc/components/video_codec/RTCVideoEncoderH264.mm` の `setBitrate:framerate:` および `setEncoderBitrateBps:frameRate:`
- nvcodec-rs: `src/encode.rs` の `ReconfigureParams` / `Encoder::reconfigure` (`nvEncReconfigureEncoder`)
- amf-rs: `src/encode.rs` の `ReconfigureParams` / `Encoder::reconfigure` (`AMFPropertyStorage`)
- vpl-rs: `src/encode.rs` の `ReconfigureParams` / `Encoder::reconfigure` (`MFXVideoENCODE_Reset`)
- 前提となる過去 issue: `issues/closed/0004-encoder-dynamic-resolution-change.md`

## 解決方法

### 実装

- `src/encoder.rs`:
  - `ReconfigureParams { average_bitrate: Option<u64>, expected_frame_rate: Option<u32> }` を `#[derive(Debug, Clone, Default)]` で追加した
  - `Encoder::reconfigure` のシグネチャを `(&mut self, params: ReconfigureParams) -> Result<(), Error>` に変更した
  - 実装を `VTSessionSetProperties` ベースに差し替えた:
    - 全項目 `None` のときは API 呼び出しを行わず `Ok(())` を返す
    - `validate_reconfigure_params` で `average_bitrate <= i64::MAX` / `expected_frame_rate != 0` / `expected_frame_rate <= i32::MAX` を検証する
    - `Some` の項目だけを `cf_dictionary` に詰めて 1 回の `VTSessionSetProperties` で反映する
    - 成功してから `self.config` を更新する (失敗時は変更しない)
    - `self.finish()` を呼ばない、`self.next_input_pts` をリセットしない、`self.callback` を維持する
  - 旧 `reconfigure` を削除した結果、`callback` フィールドへの直接参照が無くなったため `#[allow(dead_code)]` と FFI 保持目的のコメントを付けた
  - `Encoder::config(&self) -> &EncoderConfig` ゲッターを追加した
- `src/lib.rs`: `ReconfigureParams` を `pub use` で公開した

### テスト

`tests/test_encoder.rs` に以下を追加した (全 7 件パス済み):

- `reconfigure_no_op_when_all_none`: 全項目 `None` で `Ok(())` を返し設定が変化しないこと、後続の `encode` も成功することを確認
- `reconfigure_updates_average_bitrate`: `config().average_bitrate` が反映されることを確認
- `reconfigure_updates_expected_frame_rate`: 初期値 30000/1001 から `expected_frame_rate = 60` で `fps_numerator = 60`, `fps_denominator = 1` に正規化されることを確認
- `reconfigure_rejects_zero_expected_frame_rate`: `Some(0)` で `InvalidConfig`、設定が変化しないこと
- `reconfigure_rejects_expected_frame_rate_above_i32_max`: `Some(i32::MAX as u32 + 1)` で `InvalidConfig`
- `reconfigure_rejects_average_bitrate_above_i64_max`: `Some(i64::MAX as u64 + 1)` で `InvalidConfig`、設定が変化しないこと
- `reconfigure_preserves_encode_after_update`: encode 2 回 → reconfigure → encode 2 回でコールバック順序が `[1, 2, 3, 4]` 通りに維持されることを確認 (issue では `reconfigure_preserves_next_input_pts` という名で書いていたが、`next_input_pts` は private なため、コールバックの user_data 順序で PTS 連続性を観測する形に変更)

### ドキュメント

- `README.md`: 「動的フォーマット変更 → エンコーダー」節と「まとめ」表を `ReconfigureParams` ベースに書き換え、`closed/0004` 由来の「常にセッション破棄 + 再作成」記述を削除し、解像度・コーデック・ピクセルフォーマット変更は `Encoder` 再生成で行う方針を明記した
- `CHANGES.md`: `## develop` に `[ADD] ReconfigureParams 構造体` / `[ADD] Encoder::config() ゲッター` / `[CHANGE] Encoder::reconfigure を動的プロパティ更新専用 API に変更する` を追記した

### 検証

- `cargo clippy --all-targets -- -D warnings` 通過
- `cargo test` 全 23 テスト (test_encoder 22 + test_error 1) 通過
