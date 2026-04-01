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

## 2026.1.0

**リリース日**: 2026-04-01


- [ADD] `Error::InvalidConfig` バリアントを追加する
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
- [UPDATE] VP9 / AV1 デコーダーテストで `supported_codecs()` による事前チェックを行い非対応環境ではテストをスキップする
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
- [FIX] H.264 プロファイルレベルを 3.1 固定から AutoLevel に変更する
  - @voluntas
- [FIX] `average_bitrate` の CFNumber 型を i32 から i64 に変更し高ビットレートに対応する
  - @voluntas
- [FIX] `kVTCompressionPropertyKey_PixelTransferProperties` への誤った設定を削除する
  - @voluntas

### misc

- VP9 デコーダーテストを追加する
  - shiguredo_libvpx でカラーバーをエンコードし Video Toolbox でデコードして PSNR を検証する
  - @voluntas

## 2025.1.0

**リリース日**: 2025-09-26
