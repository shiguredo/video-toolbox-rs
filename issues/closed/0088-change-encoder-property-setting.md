# エンコーダーのプロパティを 1 個ずつ設定し、未指定は設定せず未対応はエラーにする

- Created: 2026-10-03
- Completed: 2026-10-05
- Branch: feature/change-encoder-property-setting
- Polished: {YYYY-MM-DD}

## 目的

`Encoder::new` は、指定したプロパティが Video Toolbox に反映されなくても `Ok` を返す。crate が使っている `VTSessionSetProperties` (辞書にまとめて 1 回) は、選択されたエンコーダーが対応していないプロパティでも `noErr` を返すためで、利用者は「設定したつもり」のまま動かすことになる。プロパティを 1 個ずつ `VTSessionSetProperty` で設定して戻り値を確認し、反映できない指定を `Encoder::new` のエラーとして検出できるようにする。あわせて、利用者が指定していないプロパティを crate の既定値で押し付けないようにする。

## 現状

- `src/encoder/session.rs` の `create_compression_session` は、`add_common_properties` / `add_h264_specific_properties` / `add_h265_specific_properties` で `(キー, 値)` を `Vec` に積み、`cf_dictionary` で辞書化して `VTSessionSetProperties` を 1 回呼ぶ。戻り値は `Error::check(status, "VTSessionSetProperties")` で確認するだけ
- `src/encoder/config.rs` の `EncoderConfig` は `real_time` / `allow_frame_reordering` / `allow_temporal_compression` を bool で持ち、常にプロパティとして設定する。`H264EncoderConfig::{profile, entropy_mode}` と `HevcEncoderConfig::{profile, allow_open_gop}` も常に設定し、`allow_open_gop` は `false` (既定) のとき `AllowOpenGOP` に `false` を設定する
- `src/error.rs` の `Error::VideoToolbox` は `status` と `function` しか持たず、どのプロパティで失敗したか分からない
- 実測 (macOS 26.5.2 / Apple M1、1920x1080)
  - 辞書にまとめた `VTSessionSetProperties` は未対応の値でも `0` を返す。`AverageBitRate` に `1e12` を指定すると複数形は `0` で `VTSessionCopyProperty` の読み戻しは既定値 (`14100480`)、単体なら `-12900`。`MaxFrameDelayCount=2` も複数形は `0` で読み戻しは `3`、単体は `-12900`
  - 単体で `-12900` になるもの: `MaxFrameDelayCount` (H.264 / HEVC の HW / SW すべて)、`PrioritizeEncodingSpeedOverQuality=true` (H.264 SW / HEVC SW)、`AllowOpenGOP=false` と `DataRateLimits` (HEVC SW)
  - 単体で `0` になるもの: `ExpectedFrameRate` / `AverageBitRate` / `RealTime` / `AllowFrameReordering` / `AllowTemporalCompression` / `MaxKeyFrameInterval` / `MaxKeyFrameIntervalDuration` / `MaximizePowerEfficiency` / `ProfileLevel` / `H264EntropyMode`
  - `VTCopySupportedPropertyDictionaryForEncoder` はプロパティの有無と `kVTPropertyReadWriteStatusKey` を返すため上記の未対応を予測できる (`MaxFrameDelayCount` は HW が `ReadOnly`、SW は未掲載)。ただし全 132 件に min/max 属性が無く値の範囲は取得できない
  - `EncoderConfig::max_frame_delay_count` は Apple Silicon の Apple エンコーダーでは設定できないため、現状は `Encoder::new` が `Ok` を返すのに反映されない

## 設計方針

- Video Toolbox のプロパティとして設定しているフィールドを `Option` にし、`None` は「そのプロパティを設定しない (Video Toolbox の既定に任せる)」とする
  - 対象: `EncoderConfig` の `prioritize_encoding_speed_over_quality` / `real_time` / `maximize_power_efficiency` / `allow_frame_reordering` / `allow_temporal_compression`、`H264EncoderConfig` の `profile` / `entropy_mode`、`HevcEncoderConfig` の `profile` / `allow_open_gop`
  - `fps_numerator` / `fps_denominator` は PTS 計算 (`CMTimeMake`) にも使うため `Option` にせず、`ExpectedFrameRate` は常に設定する
  - `data_rate_limits` は空 `Vec` が既に「設定しない」を意味するため型を変えない (issue 0076 の方針を維持)
  - 未指定時に Video Toolbox の既定値が使われることで挙動が変わるフィールドがある。`allow_frame_reordering` (Video Toolbox の既定は true、crate の現既定は false) と HEVC の `allow_open_gop` (既定は true、crate は false を設定) はビットストリームが変わるため、`CHANGES.md` の `[CHANGE]` に明記する
- プロパティは 1 個ずつ `VTSessionSetProperty` で設定し、戻り値が `0` でなければ `Encoder::new` をエラーにする。値の範囲を調べるための複数回の試行や、事前判定のための追加照会は行わない
- `Error::VideoToolbox` にプロパティ名を持たせ、どのプロパティで失敗したかを返す (公開エラーの破壊的変更)
- 明示指定したプロパティが未対応の場合はエラーにする。`max_frame_delay_count` は Apple Silicon の Apple エンコーダーでは常にエラーになるが、Video Toolbox の仕様上は read/write であり、対応するエンコーダーでは設定できるためフィールドは残し、エラーで伝える
- `Encoder::config` は指定値をそのまま返す現行契約を維持し、rustdoc に「未指定のフィールドは設定されない」「Video Toolbox が受け付けなかったプロパティは `Encoder::new` がエラーを返す」を書く

## 完了条件

- `None` のフィールドに対応するプロパティを `VTSessionSetProperty` で設定しないこと
- 明示指定したプロパティが `-12900` などのエラーを返した場合に `Encoder::new` が `Err` を返し、エラーからプロパティ名が分かること
- 実測に基づくテストが追加されていること (未指定なら設定しない / 未対応プロパティでエラー / 正常系は従来どおり)
- `EncoderConfig` / `H264EncoderConfig` / `HevcEncoderConfig` / `Error` の rustdoc、README、`skills/shiguredo-video-toolbox/SKILL.md`、`CHANGES.md` が実態に合っていること
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通ること

## 関連 issue

- issue 0076: `VTSessionSetProperties` がエンコード中の `DataRateLimits` の変更を黙殺することを実測し、契約を実態に合わせた。本 issue は同じ「Video Toolbox が未対応の設定を無視する」性質を、設定時の戻り値で検出できるようにする
- issue 0087: `create_compression_session` のセッション所有権ガードを変更する。本 issue は同じ関数のプロパティ設定部分を変更するため、先に 0087 を片付けると衝突しにくい

## 解決方法

`Encoder::new` がプロパティを 1 個ずつ設定して戻り値を確認するようにし、未指定のプロパティは設定しないようにした。

### プロパティの設定

- `create_compression_session` の `add_common_properties` / `add_h264_specific_properties` /
  `add_h265_specific_properties` を削除し、辞書化して `VTSessionSetProperties` を 1 回呼ぶ実装をやめた。
  `set_properties` が `set_property` 経由で 1 個ずつ `VTSessionSetProperty` を呼び、戻り値が `0` でなければ
  `Error::VideoToolbox` を返す。値の範囲を調べるための複数回の試行や事前照会は行わない
- `EncoderConfig` の `prioritize_encoding_speed_over_quality` / `real_time` / `maximize_power_efficiency` /
  `allow_frame_reordering` / `allow_temporal_compression`、`H264EncoderConfig` の `profile` /
  `entropy_mode`、`HevcEncoderConfig` の `profile` / `allow_open_gop` を `Option` にした。`None` の
  フィールドは `VTSessionSetProperty` の呼び出し自体を行わない
- `fps_numerator` / `fps_denominator` は `CMTimeMake` の PTS 計算にも使うため `Option` にせず、
  `ExpectedFrameRate` は常に設定する
- `Encoder::reconfigure` は 1 回の `VTSessionSetProperties` のままにした。1 個ずつに変えると一部の
  プロパティだけが反映された状態でエラーを返す可能性があり、「失敗時は `config` を変更しない」契約が
  崩れるため
- `data_rate_limits` は型を変えなかった。`VTSessionCopyProperty` で読み戻して確認したところ、未設定時の
  既定値と空配列を設定した後の状態は H.264 / HEVC の HW / SW のすべてで一致し (空配列は既存の上限を
  変えない)、HEVC の SW エンコーダーでは空配列の設定自体が `-12900` になる。`Option<Vec<DataRateLimit>>`
  にすると「`None` なら成功するのに `Some(vec![])` だと失敗する」という得るものの無いエラー経路が
  増えるだけだった
- 空 `Vec` を「上限なし」と説明していた rustdoc と README を「本プロパティを設定しない (Video Toolbox の
  既定に任せる)」に直した。H.264 の SW エンコーダーでは既定で 2 個のリミットが設定されており、
  「設定しない」は「上限なし」を保証しない

### エラー

- `Error::VideoToolbox` に `property: Option<String>` を追加した。`VTSessionSetProperty` の失敗では
  受け付けられなかったプロパティ名が入り、プロパティを指定しない呼び出しの失敗では `None` になる
- 表示は `[shiguredo_video_toolbox] VTSessionSetProperty(kVTCompressionPropertyKey_MaxFrameDelayCount)
  failed: status=-12900` のようにプロパティ名を含む

### テスト

- `encoder_reports_property_rejected_by_video_toolbox` を追加した。`max_frame_delay_count` を `None` に
  すると同じプロパティでも構築に成功し、`Some(2)` にすると `-12900` で失敗して `property` から原因の
  プロパティ名が取れることを検証する (macOS 26.5.2 / Apple M1 の実測)
- `encode_h264_reports_b_frames_when_frame_reordering_unspecified` を追加した。
  `allow_frame_reordering` を `None` にすると Video Toolbox の既定 (フレーム再順序付け有効) で
  B フレームが生成されることを検証する。既存の `assert_picture_type_and_timestamp` を `Option<bool>` に
  拡張して `None` の場合も扱うようにした
- `Error::VideoToolbox` の `property` を含む表示を `tests/test_error.rs` で検証するようにした
- 設定を構築している箇所 (`src` / `tests` / `pbt` / `examples`) を `Option` に追従させた

### その他

- 完了条件のコマンド (`cargo test --workspace -- --test-threads=1` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check`) が通ることを確認した
- CHANGES.md の `## develop` に `[CHANGE]` 2 件と `### misc` に `[UPDATE]` 1 件を追加した
