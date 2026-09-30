# EncodedFrame にタイムスタンプとピクチャータイプを追加する

- Created: 2026-09-30
- Completed: {YYYY-MM-DD}
- Branch: feature/add-encoded-frame-timestamp
- Polished: {YYYY-MM-DD}

## 目的

`EncodedFrame` が持つ情報が「キーフレームかどうか」と圧縮データだけであり、フレームの提示時刻（PTS）とピクチャータイプを利用側が取得できない。この 2 つは MP4 などへ多重化する際に必須で、取得できないと次が壊れる。

- PTS が無いため、各サンプルの提示時刻を利用側が自前で積算するしかない。エンコード中にフレームレートを `Encoder::reconfigure` で変更した場合や、Video Toolbox がフレームをドロップした場合に、実際の時刻とずれる
- ピクチャータイプが `keyframe: bool` だけでは I フレームと IDR フレームを区別できず、B フレームの存在も分からない。そのため MP4 の `ctts`（composition time offset）を正しく書けず、B フレームを含むストリームの再生時刻がずれる

一方、`EncodedFrame` を受け取るエンコードコールバックは `CMSampleBufferRef` を引数に持っており、必要な情報はすべてそこから取得できる。現状は `output_callback_h264` / `output_callback_h265` が時刻引数（`_presentation_time_stamp` / `_presentation_duration`）を未使用のまま捨てており、`CMSampleBufferGetPresentationTimeStamp` なども呼んでいない。取得できる情報を取りこぼしている状態を解消する。

もう一方のバックエンドである nvcodec-rs の `EncodedFrame` は `timestamp()` と `picture_type()` を持ち、`PictureType` で P / B / I / IDR / BI / Skipped / IntraRefresh / NonRefP / Switch を区別する。本クレートにも同等の情報を追加し、4 バックエンドで語彙を揃える。

## 現状

- `src/encoder/frame.rs` の `EncodedFrame` は `keyframe` / `sps_list` / `pps_list` / `vps_list` / `data` / `user_data` を公開フィールドとして持つ。時刻とピクチャータイプのフィールドは無い
- `src/encoder/callback.rs` の `output_callback_h264` / `output_callback_h265` は `_presentation_time_stamp` / `_presentation_duration` を受け取るが未使用
- `src/encoder/callback.rs` の `is_keyframe` は `kCMSampleAttachmentKey_NotSync` の有無だけを見ており、`kCMSampleAttachmentKey_DependsOnOthers` / `kCMSampleAttachmentKey_IsDependedOnByOthers` は読んでいない
- `examples/raden_to_mp4.rs` は `Sample { duration: 1, composition_time_offset: None, .. }` を固定値で組み立てており、エンコーダーが返す時刻を使っていない
- 入力側の PTS は `Encoder` が `next_input_pts` を `fps_denominator` ずつ加算して自動生成しており、利用者が指定する API は無い（この点は本 issue の対象外とする）

## 設計方針

### 時刻

- `EncodedFrame` に `timestamp` を追加する。値は `CMSampleBufferGetPresentationTimeStamp` で取得した `CMTime` を、エンコーダーが入力 PTS に使っている timescale（`EncoderConfig::fps_numerator`）に揃えて表現する
- `CMTime` の `flags` を確認し、有効でない時刻（`kCMTimeFlags_Valid` が立っていない、または不定・無限）の場合は時刻を返さない。この場合に 0 を返すと「先頭フレーム」と区別できないため、`Option` で表現するか、無効であることを示す別の表現にする。どちらを採るかは実装時に確定し、rustdoc に契約を書く
- 型は nvcodec-rs の `EncodedFrame::timestamp() -> u64` と揃えるか、`CMTime` 相当の値をそのまま返すかを実装時に確定する。揃える場合は timescale を同時に返す必要があるか検討すること（`u64` だけでは時刻の解釈に timescale が要る）
- `Encoder::reconfigure` で `expected_frame_rate` を変更した場合、`next_input_pts` が新しい timescale に再スケールされる。取得する時刻はその時点の timescale に従うため、利用側が複数の timescale を跨いで時刻を扱えるようにする必要がある

### ピクチャータイプ

- `PictureType` を追加する。バリアントは nvcodec-rs の `PictureType` と揃えることを基本とし、Video Toolbox から区別できないものはその旨を rustdoc に明記する
- Video Toolbox から取得できる情報は次のとおり
  - `kCMSampleAttachmentKey_NotSync`: 同期サンプル（キーフレーム）かどうか
  - `kCMSampleAttachmentKey_DependsOnOthers`: 他のフレームを参照するか（I フレーム判定）
  - `kCMSampleAttachmentKey_IsDependedOnByOthers`: 他のフレームから参照されるか（使い捨てフレーム判定）
  - `kCMSampleAttachmentKey_PartialSync`: 部分同期かどうか
- 上記から I / IDR / P / B などをどこまで一意に決められるかは、実際の出力で添付情報を観測して確定する。推測でマッピングを決めないこと。確定できない区別（例: I と IDR）は同じバリアントにまとめるか、区別できないことを示すバリアント（nvcodec-rs の `Unknown` に相当）で表現する
- 既存の `keyframe: bool` は同期サンプルかどうかを表す。`PictureType` を追加しても `keyframe` は残すか統合するかを実装時に確定する。統合する場合は CHANGES.md の `[CHANGE]` に記載すること
- B フレームの有無は `kCMSampleAttachmentKey_DependsOnOthers` だけでは決まらない。DTS（`CMSampleBufferGetDecodeTimeStamp`）と PTS の順序関係を使う方法を検討し、採用する場合はその根拠をコメントに残す

### 添付情報の取得

`src/encoder/callback.rs` の `is_keyframe` は添付配列を取得して `kCMSampleAttachmentKey_NotSync` だけを読む。ピクチャータイプ判定で複数のキーを読むため、添付辞書の取得と型チェックを 1 か所にまとめ、各キーを読むヘルパーに分けること。既存の「添付が CFDictionary でない場合は解釈しない」という防御は維持する。

## 完了条件

- `EncodedFrame` からエンコード結果の時刻が取得でき、`Encoder::reconfigure` でフレームレートを変更した後も時刻が単調増加することをテストで検証する
- `PictureType` が取得でき、H.264 / H.265 のそれぞれで `allow_frame_reordering` を有効にした場合と無効にした場合で妥当な値になることをテストで検証する
- 時刻が無効な場合（`CMTime` が有効でない場合）の契約が rustdoc に明記され、テストで検証されている
- `examples/raden_to_mp4.rs` が取得した時刻を使ってサンプルを組み立てるようにする（固定値の積算をやめる）
- README の `EncodedFrame` に関する記述を更新する
- CHANGES.md に `[ADD]` エントリ（破壊的変更を伴う場合は `[CHANGE]` も）を追加する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 解決方法

未着手。
