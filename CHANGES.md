# 変更履歴

- UPDATE
  - 後方互換がある変更
- ADD
  - 後方互換がある追加
- CHANGE
  - 後方互換のない変更
- FIX
  - バグ修正

## develop

- [CHANGE] MSRV (rust-version) を 1.93 に上げる
  - @voluntas
- [UPDATE] ログ出力のクレートを `log` から `tracing` に切り替える
  - shiguredo-rust 規約の「ログは tracing を使うこと」に合わせる
  - `log::error!` を `tracing::error!` に置換し、`Cargo.toml` の依存を `tracing = "0.1"` に差し替える
  - @voluntas
- [ADD] `kVTCompressionPropertyKey_DataRateLimits` に対応する `DataRateLimit` 型と
  `ReconfigureParams::data_rate_limits` を追加する
  - `AverageBitRate` 指定だけでは短期ウィンドウで大きくオーバーシュートするため、
    ウィンドウあたりの総バイト数のハード上限を併設できるようにする
  - 指定できるリミットは Video Toolbox の仕様上 0〜2 個
  - @voluntas
- [ADD] `Encoder::config` で現在保持している `EncoderConfig` を参照する getter を追加する
  - @voluntas
- [CHANGE] `Encoder` と `Decoder` をコールバックベースの非同期 API に変更する
  - @melpon
- [CHANGE] `Encoder::reconfigure` を `ReconfigureParams` ベースの動的更新専用 API に変更する
  - 旧 API は `EncoderConfig` を所有権で受け取りセッションを再作成していたが、
    `VTSessionSetProperties` 1 回で完結する動的更新型に置き換える
  - 動的に変更可能な項目は `average_bitrate` / `expected_frame_rate` / `data_rate_limits` の
    3 項目で、解像度・コーデック・ピクセルフォーマットの変更は `Encoder` を作り直す運用に統一する
  - `expected_frame_rate` 更新時は内部 PTS を切り上げで再スケールし、
    オーバーフロー時には `Error::LimitExceeded` を返す
  - @voluntas
- [CHANGE] `EncoderConfig` に `data_rate_limits` フィールドを追加する
  - `#[non_exhaustive]` を付けていない公開構造体へのフィールド追加のため、
    構造体リテラルで構築しているコードは `data_rate_limits: None` の追記が必要になる
  - @voluntas
- [CHANGE] `Encoder::new` が `average_bitrate` に 0 を指定された場合に `Error::InvalidConfig` を返すようにする
  - 従来は 0 がそのまま Video Toolbox に渡っていたが、`Encoder::reconfigure` と検証を共通化して
    構築時点で拒否する
  - @voluntas
- [CHANGE] `Error` に `UnknownPixelFormat` バリアントを追加する
  - `Encoder::encode_pixel_buffer` に I420 / Nv12 のいずれでもない FourCC が渡された場合に、
    `PixelFormatMismatch` と区別して実際の FourCC を診断情報として返す
  - `Error` は `#[non_exhaustive]` ではないため、網羅 `match` している場合は分岐の追加が必要になる
  - @voluntas
- [CHANGE] `Error` 型の全 `&'static str` フィールドを `String` に変更し、エラーメッセージに動的な値を含められるようにする
  - `Option` を返していた内部関数を `Result<_, Error>` に変更し、エラー情報をコールバック経由でユーザーに伝搬する
  - 不要になった `tracing::error!()` を削除する
  - @melpon
- [FIX] 圧縮セッションのプロパティ設定が失敗したときにセッションがリークする問題を修正する
  - `VTCompressionSessionCreate` 成功後のセッションを `CfPtrMut` でガードし、エラーパスでは `Drop` が
    `CFRelease` で解放する
  - 成功パスでは `forget` でガードを解除し、`Encoder::drop` の解放処理との二重解放を防ぐ
  - @voluntas

### misc

- [UPDATE] `encoder_rejects_*` と `reconfigure_is_noop_when_all_none` のテストアサーションを強化する
  - `encoder_rejects_*` 系が `Error::InvalidConfig` の `reason` 文字列まで検証するようにする
  - `reconfigure_is_noop_when_all_none` が no-op 後に `encode` が成功することも検証するようにする
  - @voluntas
- [UPDATE] `Encoder::reconfigure` の片肺更新 (bitrate のみ / fps のみ) の単体テストを追加する
  - 片肺更新時に「対側が不変」「更新側が反映」と fps 更新時の `fps_denominator = 1` 正規化を検証する
  - @voluntas
- [UPDATE] `Encoder::config` getter の単体テストを追加する
  - `Encoder::new` 直後の `config()` が入力した全フィールドを変更なしで返すことを検証する
    (`Some(空 Vec)` の `data_rate_limits` が `None` に正規化されるケースを除く)
  - @voluntas
- [UPDATE] `average_bitrate` / `expected_frame_rate` の CFNumber 構築と push をヘルパー関数に共通化する
  - `push_bitrate_property` / `push_expected_frame_rate_property` を追加し、
    `add_common_properties` と `reconfigure` の 2 箇所の重複を解消する
  - `push_data_rate_limits_property` と同じ構成のモジュールレベルフリー関数とし、
    `unsafe` はヘルパー内部のキー参照に閉じ込める
  - 挙動は変わらない
  - @voluntas
- [UPDATE] `fps_numerator` と `expected_frame_rate` の検証ロジックを共通関数 `validate_positive_i32_field` に統合する
  - ゼロ拒否と `i32::MAX` 上限拒否が 2 関数に重複していたため、`field` / `reason_overflow` を引数化して 1 本に集約する
  - エラーメッセージは従来と同一で、挙動は変わらない
  - @voluntas
- [UPDATE] `Encoder::reconfigure` 関連の rustdoc を整理して説明の重複を解消する
  - 「動的に更新できる項目 / できない項目」の本体説明を `Encoder::reconfigure` に集約し、
    `ReconfigureParams` / `Encoder::config` からは参照で辿れるようにする
  - @voluntas

## 2026.1.1

**リリース日**: 2026-04-01

- [UPDATE] `Encoder` の設定検証・プレーンコピー・パラメータ抽出・ドキュメントを堅牢化する
  - `fps_numerator` が `i32::MAX` を超える場合は `InvalidConfig` とする（`CMTimeMake` の timescale 用）
  - `copy_plane` で CVPixelBuffer のプレーン寸法・格納バイト数とコピー範囲を照合する
  - パラメータセット抽出で異常に大きい長さを拒否する（ISO/IEC 14496-15 の `unsigned int(16)` に合わせ 65535 バイト上限と根拠コメント）
  - `Encoder` と `next_frame` のドキュメントを補う（無制限 mpsc・出力 PTS 列のギャップ）
  - 同一 PTS のエンコード出力が上書きされたときにログする
  - @voluntas


## 2026.1.0

**リリース日**: 2026-04-01

- [UPDATE] VP9 / AV1 デコーダーテストで `supported_codecs()` による事前チェックを行い非対応環境ではテストをスキップする
  - @voluntas
- [ADD] `Error::CfObjectCreationFailed` バリアントを追加し、CF の辞書・数値生成が NULL を返した場合を `LimitExceeded` と区別する
  - @voluntas
- [ADD] `Error::InvalidConfig` バリアントを追加する
  - @voluntas
- [ADD] コーデック情報取得 API `supported_codecs()` を追加する
  - `VideoCodecType`, `CodecInfo`, `DecodingInfo`, `EncodingInfo` 型を追加する
  - デコード判定に `VTIsHardwareDecodeSupported` を使用する
  - エンコード判定に `VTCopyVideoEncoderList` を使用する
  - @voluntas
- [ADD] `Error::UnsupportedCodec` バリアントを追加する
  - VP9 / AV1 デコーダーが環境で利用できない場合に明確なエラーを返す
  - @voluntas
- [ADD] `Error::InsufficientFrameData` バリアントを追加する
  - @voluntas
- [ADD] `Encoder::reconfigure()` メソッドを追加する
  - @voluntas
- [ADD] `Decoder::update_format()` メソッドを追加する
  - @voluntas
- [ADD] `DecoderConfig` 構造体を追加する
  - @voluntas
- [ADD] `FrameData` enum を追加する
  - @voluntas
- [ADD] `PixelFormat` enum を追加する
  - @voluntas
- [ADD] `EncodeOptions` struct を追加し、`force_key_frame` でキーフレーム生成を強制可能にする
  - @voluntas
- [CHANGE] `Encoder::next_frame` の戻り値を `Result<Option<EncodedFrame>, Error>` にし、出力 PTS の `checked_add` 失敗時は `Error::LimitExceeded` を返す
  - `checked_add` の可否は `output_frames` から取り出す前に検証し、オーバーフロー時にエンコード済みフレームを失わない
  - `try_recv()` で受信がなくても、バッファ済みの `output_frames` から `next_output_pts` 一致分を返す
  - @voluntas
- [CHANGE] `Error::LimitExceeded` バリアントを追加する
  - CF オブジェクト生成失敗・PTS 加算オーバーフロー・プレーンコピー算術オーバーフロー等で返す
  - @voluntas
- [CHANGE] `EncoderConfig` に `pixel_format` フィールドを追加する
  - @voluntas
- [CHANGE] `Error` を enum に変更し `PixelFormatMismatch` バリアントを追加する
  - @voluntas
- [CHANGE] `DecodedFrame` を `I420Frame` / `Nv12Frame` の enum に変更する
  - @voluntas
- [CHANGE] `Decoder::new()` の引数を `DecoderConfig` に変更する
  - @voluntas
- [CHANGE] `Encoder::encode()` の引数を `FrameData` enum に変更する
  - @voluntas
- [CHANGE] `Encoder::encode()` に `options: &EncodeOptions` 引数を追加する
  - @voluntas
- [CHANGE] `Decoder::new_h264()` / `new_h265()` / `new_vp9()` / `new_av1()` を廃止し `Decoder::new(codec: DecoderCodec)` に統合する
  - @voluntas
- [CHANGE] `DecoderCodec` enum を追加し、コーデック種別と初期化パラメータを一体化する
  - @voluntas
- [CHANGE] `Decoder` の `nalu_len_bytes` / `width` / `height` を `usize` から `u32` に変更する
  - @voluntas
- [CHANGE] `ProfileLevel` enum を廃止し、コーデック固有のプロファイル enum に置き換える
  - `H264Profile` (Baseline / Main / High) と `HevcProfile` (Main / Main10) を追加する
  - @voluntas
- [CHANGE] `Encoder::new_h264()` / `Encoder::new_h265()` を廃止し `Encoder::new()` に統合する
  - `EncoderConfig` に `codec: CodecConfig` フィールドを追加し、コーデック種別とコーデック固有設定を一体化する
  - @voluntas
- [CHANGE] `EncoderConfig` から `h264_entropy_mode` と `allow_open_gop` フィールドを削除する
  - コーデック固有設定構造体 (`H264EncoderConfig` / `HevcEncoderConfig`) に移動する
  - @voluntas
- [CHANGE] `EncoderConfig` の `Default` 実装を削除する
  - 全フィールド明示を必須にする
  - @voluntas
- [CHANGE] `EncoderConfig` の整数型を `usize` から `u32` に変更する
  - `width` / `height` / `fps_numerator` / `fps_denominator` を `u32` に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` の `target_bitrate` を `usize` から `Option<u64>` に変更する
  - 未指定時はバックエンド依存とする
  - @voluntas
- [CHANGE] `EncoderConfig` の `max_key_frame_interval` / `max_frame_delay_count` を `NonZeroUsize` から `NonZeroU32` に変更する
  - @voluntas
- [CHANGE] `Encoder::new()` の引数を `&EncoderConfig` からムーブ (`EncoderConfig`) に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` のフィールド名を Video Toolbox のプロパティ名に合わせてリネームする
  - `target_bitrate` を `average_bitrate` に変更する
  - `prioritize_speed_over_quality` を `prioritize_encoding_speed_over_quality` に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` から未使用の `use_parallelization` フィールドを削除する
  - @voluntas
- [FIX] `codec_info` で Core Foundation の参照を `CFGetTypeID` なしで辞書・数値・配列・文字列として扱っていた箇所を型検証する
  - @voluntas
- [FIX] `Encoder::validate_config` で `width` / `height` が 0 の場合を拒否し、`validate_frame_data` の解像度乗算に `checked_mul` を用いる
  - @voluntas
- [FIX] `copy_plane` でプレーン基底アドレスが NULL の場合はエラーにし、`bytes_per_row` がコピー幅より小さい場合はエラーにする（行バッファ外書き込みの防止）
  - @voluntas
- [FIX] `process_encoded_output` でキーフレーム時に `CMSampleBufferGetFormatDescription` が NULL の場合は打ち切る
  - @voluntas
- [FIX] `is_keyframe` で添付が `CFDictionary` であることを `CFGetTypeID` で確認する
  - @voluntas
- [FIX] パラメータ抽出で `CMVideoFormatDescriptionRef` が NULL の場合は打ち切る
  - @voluntas
- [FIX] `Decoder::decode` と `I420Frame` / `Nv12Frame` のドキュメントを補足する
  - @voluntas
- [FIX] VP9 ラウンドトリップテストで libvpx の失敗時はテストを打ち切る
  - @voluntas
- [FIX] `Decoder::decode` で圧縮データを `Vec` にコピーしてから `CMBlockBufferCreateWithMemoryBlock` に渡す
  - @voluntas
- [FIX] `cf_dictionary` / `cf_number_*` の戻り NULL を `LimitExceeded` として扱う
  - @voluntas
- [FIX] `Encoder` の入力 / 出力 PTS を `checked_add` で更新し、オーバーフロー時はエラーまたはログする
  - @voluntas
- [FIX] `copy_plane` で `checked_mul` によりコピー長・行オフセットのオーバーフローを検出する
  - @voluntas
- [FIX] `I420Frame` / `Nv12Frame` のプレーン参照で NULL および算術オーバーフロー時は空スライスとする
  - @voluntas
- [FIX] `CMSampleBufferGetDataBuffer` が NULL のときはエンコード出力処理を行わない
  - @voluntas
- [FIX] `CMBlockBufferGetDataPointer` の結果を `vec_u8_from_raw_parts_safe` 経由でコピーする
  - @voluntas
- [FIX] `is_keyframe` で `CFArray` の要素数を確認する
  - @voluntas
- [FIX] `Encoder::validate_config` で `fps_numerator` が 0 の場合を拒否する
  - @voluntas
- [FIX] H.264 / H.265 のパラメータセット抽出でポインタとサイズの組み合わせを検証する
  - @voluntas
- [FIX] `Encoder::encode()` で必要サイズより長い入力スライスを渡した場合にヒープオーバーランが発生する問題を修正する
  - `copy_plane()` のコピーサイズを `src.len()` から `src_width * src_height` に変更する
  - @voluntas
- [FIX] `Encoder::encode()` で入力フレームのメモリ寿命が非同期エンコード中に保証されない問題を修正する
  - `CVPixelBufferCreateWithPlanarBytes` を `CVPixelBufferCreate` + データコピーに変更する
  - @voluntas
- [FIX] `fps_denominator` が 0 の場合にゼロ除算パニックが発生する問題を修正する
  - `Encoder::new()` と `Encoder::reconfigure()` で設定値を検証する
  - @voluntas
- [FIX] エンコーダーとデコーダーのコールバックで NULL 出力を考慮していない問題を修正する
  - @voluntas
- [FIX] H.264 プロファイルレベルを 3.1 固定から AutoLevel に変更する
  - @voluntas
- [FIX] `average_bitrate` の CFNumber 型を i32 から i64 に変更し高ビットレートに対応する
  - @voluntas
- [FIX] `kVTCompressionPropertyKey_PixelTransferProperties` への誤った設定を削除する
  - @voluntas
- [FIX] `Encoder::encode()` で `copy_plane` が失敗したときに `CVPixelBufferUnlockBaseAddress` が呼ばれずロックしたまま `CFRelease` され得る問題を修正する
  - ロック解除を `Drop` ガードで保証する
  - @voluntas
- [FIX] エンコードコールバックで `CMBlockBufferGetDataPointer` の戻り長だけを出力長に使うと非連続 `CMBlockBuffer` で圧縮データが途中までしか取れない問題を修正する
  - `CMBlockBufferCopyDataBytes` で `CMBlockBufferGetDataLength` 分をコピーする
  - @voluntas
- [UPDATE] README にテスト前提を記載し、`DecodingInfo`・`supported_codecs`・`DecodedFrame` / `I420Frame` / `Nv12Frame` の rustdoc を補う
  - @voluntas
- [FIX] `Encoder::encode` で `VTCompressionSessionEncodeFrame` 呼び出し前に `CVPixelBuffer` をアンロックする
  - @voluntas
- [FIX] エンコード出力で `CMBlockBufferGetDataLength` が防御的上限を超える場合はログして当該フレームを破棄する
  - @voluntas
- [FIX] `Encoder::validate_config` で `width` / `height` が `i32::MAX` を超える場合を拒否し、`DecoderCodec::Vp9` / `Av1` の `CMVideoFormatDescriptionCreate` 呼び出し前に同じ寸法範囲を検証する
  - `Decoder::wrap_unsupported_codec_error` は `VideoToolbox` エラーのみ `UnsupportedCodec` に変換し、`InvalidConfig` はそのまま返す
  - @voluntas
- [FIX] `EncoderConfig` の `average_bitrate` が `i64::MAX` を超える `u64` のときに `CFNumber` へ負の値が渡るのを防ぐため、`InvalidConfig` で拒否する
  - @voluntas

### misc

- CI の `release/**` ブランチ push でも GitHub Actions を実行する
  - @voluntas
- README に VP9 / AV1 デコード初期化時の `UnsupportedCodec` と `InvalidConfig` の違いを追記する
  - @voluntas
- VP9 デコーダーテストを追加する
  - shiguredo_libvpx でカラーバーをエンコードし Video Toolbox でデコードして PSNR を検証する
  - @voluntas

## 2025.1.0

**リリース日**: 2025-09-26
