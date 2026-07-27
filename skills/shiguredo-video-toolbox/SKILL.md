---
name: shiguredo-video-toolbox
description: 時雨堂の Apple Video Toolbox バインディング shiguredo_video_toolbox の機能・API リファレンス。H.264 / H.265 ハードウェアエンコード、H.264 / H.265 / VP9 / AV1 ハードウェアデコード、AVCC 形式の入出力、I420 / NV12 ピクセルフォーマット、コールバックベースの非同期 API、動的解像度変更、ゼロコピー入力に関する質問時に使用。
---

# shiguredo_video_toolbox

Apple の [Video Toolbox](https://developer.apple.com/documentation/videotoolbox) を利用したハードウェアビデオエンコーダー / デコーダーの Rust バインディング。

## 特徴

- **macOS 専用**: ビルド時に Xcode の SDK ヘッダーを参照して bindgen でバインディングを自動生成 (`target_os = "macos"` 以外では `compile_error!`)
- **ハードウェアエンコード**: H.264 / H.265
- **ハードウェアデコード**: H.264 / H.265 / VP9 / AV1 (環境依存)
- **コーデック固有設定の型安全な分離**: エンコード側は `CodecConfig` enum、デコード側は `DecoderCodec` enum
- **ピクセルフォーマット**: `PixelFormat::I420` / `PixelFormat::Nv12`
- **コールバックベースの非同期 API**: `EncodeHandler` / `DecodeHandler` トレイト
- **動的解像度変更**:
  - エンコーダー: `Encoder::reconfigure()` (常にセッション破棄 + 再作成)
  - デコーダー: `Decoder::update_format()` (受け入れ可否判定 → 必要時のみ再作成)
- **ゼロコピー入力**: `Encoder::encode_pixel_buffer()` で `CVPixelBuffer` を直接受け取る
- **AVCC 形式の入出力**: NAL ユニット長プレフィックス付き

## バージョン情報

- crate 名: `shiguredo_video_toolbox`
- バージョン: 2026.1.1
- Rust Edition: 2024
- 最小 Rust バージョン: 1.93
- ライセンス: Apache-2.0

## 動作要件

- macOS (arm64)
- Xcode Command Line Tools (ビルド時に Video Toolbox のヘッダーファイルが必要)
- `DOCS_RS=1 cargo doc --no-deps` で Xcode のない環境でもドキュメント生成は可能

## コア API

### コーデック共通型

| 型 | 説明 | 主要メソッド / 値 |
|----|------|------------------|
| `PixelFormat` | ピクセルフォーマット | `I420` (kCVPixelFormatType_420YpCbCr8Planar, 3 プレーン), `Nv12` (kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, 2 プレーン) |
| `VideoCodecType` | コーデック種別 | `H264`, `Hevc`, `Vp9`, `Av1` |

### エンコード用

| 型 | 説明 | 主要メソッド / 値 |
|----|------|------------------|
| `Encoder<H: EncodeHandler>` | H.264 / H.265 エンコーダー | `new(config, handler)`, `encode(frame, options, user_data)`, `encode_pixel_buffer(ptr, options, user_data)` (unsafe), `reconfigure(config)`, `finish()` |
| `EncoderConfig` | エンコーダー設定 | `width`, `height`, `codec`, `pixel_format`, `average_bitrate`, `fps_numerator`, `fps_denominator`, `prioritize_encoding_speed_over_quality`, `real_time`, `maximize_power_efficiency`, `allow_frame_reordering`, `allow_temporal_compression`, `max_key_frame_interval`, `max_key_frame_interval_duration`, `max_frame_delay_count` |
| `CodecConfig` | コーデック種別 + 固有設定 | `H264(H264EncoderConfig)`, `Hevc(HevcEncoderConfig)` |
| `H264EncoderConfig` | H.264 固有設定 | `profile: H264Profile`, `entropy_mode: H264EntropyMode` |
| `H264Profile` | H.264 プロファイル | `Baseline`, `Main`, `High` |
| `H264EntropyMode` | H.264 エントロピー符号化 | `Cavlc` (高速), `Cabac` (高品質) |
| `HevcEncoderConfig` | H.265 固有設定 | `profile: HevcProfile`, `allow_open_gop: bool` |
| `HevcProfile` | H.265 プロファイル | `Main`, `Main10` |
| `EncodeOptions` | フレーム単位のエンコードオプション | `force_key_frame: bool` (`Default` あり) |
| `FrameData<'a>` | 入力フレームデータ (借用) | `I420 { y, u, v }`, `Nv12 { y, uv }` |
| `EncodedFrame<T>` | エンコード結果 (AVCC 形式) | `keyframe: bool`, `sps_list: Vec<Vec<u8>>`, `pps_list: Vec<Vec<u8>>`, `vps_list: Vec<Vec<u8>>` (H.265 のみ), `data: Vec<u8>`, `user_data: T` |
| `EncodeHandler` | エンコード結果通知トレイト | `type UserData`, `type Error: From<crate::Error>`, `on_encoded(result: Result<EncodedFrame<UserData>, Error>)` |
| `FnEncodeHandler<T, E>` | `FnMut(Result<EncodedFrame<T>, E>)` ラッパー | `new(f)` |

`EncoderConfig` には `Default` 実装がない (全フィールド明示が必須)。

### デコード用

| 型 | 説明 | 主要メソッド / 値 |
|----|------|------------------|
| `Decoder<H: DecodeHandler>` | H.264 / H.265 / VP9 / AV1 デコーダー | `new(config, handler)`, `decode(data, user_data)`, `update_format(codec)`, `finish()` |
| `DecoderConfig<'a>` | デコーダー設定 | `codec: DecoderCodec<'a>`, `pixel_format: PixelFormat` |
| `DecoderCodec<'a>` | コーデック + 初期化パラメータ | `H264 { sps, pps, nalu_len_bytes }`, `Hevc { vps, sps, pps, nalu_len_bytes }`, `Vp9 { width, height }`, `Av1 { width, height }` |
| `DecodedFrame<T>` | デコード結果 | `I420 { frame: I420Frame, user_data: T }`, `Nv12 { frame: Nv12Frame, user_data: T }` |
| `I420Frame` | I420 形式の出力フレーム | `y_plane()`, `u_plane()`, `v_plane()`, `y_stride()`, `u_stride()`, `v_stride()`, `width()`, `height()` |
| `Nv12Frame` | NV12 形式の出力フレーム | `y_plane()`, `uv_plane()`, `y_stride()`, `uv_stride()`, `width()`, `height()` |
| `DecodeHandler` | デコード結果通知トレイト | `type UserData`, `type Error: From<crate::Error>`, `on_decoded(result: Result<DecodedFrame<UserData>, Error>)` |
| `FnDecodeHandler<T, E>` | `FnMut(Result<DecodedFrame<T>, E>)` ラッパー | `new(f)` |

`Decoder::decode` に渡す圧縮データは AVCC 形式 (NAL ユニットの先頭に `nalu_len_bytes` バイトの長さフィールドが付く形式)。Annex B 形式は別途変換が必要。

### コールバックの実行スレッド

`EncodeHandler::on_encoded` / `DecodeHandler::on_decoded` は **Video Toolbox のコールバックスレッド**から呼び出される。`encode()` / `decode()` を呼んだスレッドではない。完了通知をメインスレッドで処理したい場合は `std::sync::mpsc` などでメッセージを受け流すこと (サンプル `examples/raden_to_mp4.rs` を参照)。

### コーデック情報取得

| 型 / 関数 | 説明 |
|-----------|------|
| `supported_codecs() -> Vec<CodecInfo>` | 実行環境で利用可能なコーデック情報の一覧を返す (macOS のみ) |
| `CodecInfo` | `codec`, `decoding: DecodingInfo`, `encoding: EncodingInfo` |
| `DecodingInfo` | `supported`, `hardware_accelerated` (`VTIsHardwareDecodeSupported` ベース) |
| `EncodingInfo` | `supported`, `hardware_accelerated`, `supports_frame_reordering`, `supports_multi_pass`, `profiles: EncodingProfiles` |
| `EncodingProfiles` | `H264(Vec<H264EncodingProfile>)`, `Hevc(Vec<HevcEncodingProfile>)`, `None` |
| `H264EncodingProfile` | `Baseline`, `ConstrainedBaseline`, `Main`, `High`, `ConstrainedHigh` |
| `HevcEncodingProfile` | `Main`, `Main10`, `Main42210` |

エンコード判定には `VTCopyVideoEncoderList` と `VTCopySupportedPropertyDictionaryForEncoder` を使用する。プロファイル照会は 1920x1080 を代表値として行うため、解像度固有の制約は反映されない。

## エラー型

`Error` 型は以下のバリアントを持つ。

| バリアント | 説明 |
|-----------|------|
| `VideoToolbox { status, function }` | Video Toolbox API のエラー (関数名と status コード) |
| `PixelFormatMismatch { expected, actual }` | エンコーダーの `pixel_format` と入力 `FrameData` のフォーマット不一致 |
| `InsufficientFrameData { plane, expected, actual }` | フレームデータのサイズ不足 (プレーン名と必要バイト数) |
| `UnsupportedCodec { codec }` | VP9 / AV1 が環境で利用できない場合など |
| `InvalidConfig { field, reason }` | `width` / `height` / `fps_numerator` / `fps_denominator` / `average_bitrate` 等の不正値 |
| `LimitExceeded { reason }` | PTS 加算オーバーフロー、プレーンコピー算術オーバーフロー、CMBlockBuffer 長が防御的上限超過など |
| `CfObjectCreationFailed { function }` | `CFDictionaryCreate` / `CFNumberCreate` 等が NULL を返した |

`Error` は `std::error::Error` と `std::fmt::Display` を実装している。

## コード例

### エンコード

```rust
use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error, FnEncodeHandler,
    FrameData, H264EncoderConfig, H264EntropyMode, H264Profile, PixelFormat,
};

let config = EncoderConfig {
    width: 1920,
    height: 1080,
    codec: CodecConfig::H264(H264EncoderConfig {
        profile: H264Profile::Main,
        entropy_mode: H264EntropyMode::Cabac,
    }),
    pixel_format: PixelFormat::I420,
    average_bitrate: Some(5_000_000),
    fps_numerator: 30,
    fps_denominator: 1,
    prioritize_encoding_speed_over_quality: false,
    real_time: false,
    maximize_power_efficiency: false,
    allow_frame_reordering: false,
    allow_temporal_compression: true,
    max_key_frame_interval: None,
    max_key_frame_interval_duration: None,
    max_frame_delay_count: None,
};

let mut encoder = Encoder::new(
    config,
    FnEncodeHandler::new(|result: Result<EncodedFrame<u64>, Error>| {
        match result {
            Ok(encoded) => {
                println!(
                    "encoded bytes: {} keyframe={} user_data={}",
                    encoded.data.len(),
                    encoded.keyframe,
                    encoded.user_data
                );
                // encoded.sps_list / pps_list はキーフレーム時のみ非空
            }
            Err(e) => eprintln!("encode callback error: {e}"),
        }
    }),
)?;

// I420 フレームをエンコード
let frame = FrameData::I420 { y: &y_plane, u: &u_plane, v: &v_plane };
encoder.encode(&frame, &EncodeOptions::default(), 0)?;

// キーフレームを強制
encoder.encode(&frame, &EncodeOptions { force_key_frame: true }, 1)?;

// 残りのフレームをフラッシュ
encoder.finish()?;
```

### デコード

```rust
use shiguredo_video_toolbox::{
    DecodedFrame, Decoder, DecoderCodec, DecoderConfig, Error, FnDecodeHandler, PixelFormat,
};

// H.264 デコーダー (SPS / PPS が必要)
let mut decoder = Decoder::new(
    DecoderConfig {
        codec: DecoderCodec::H264 {
            sps: &sps,
            pps: &pps,
            nalu_len_bytes: 4,
        },
        pixel_format: PixelFormat::I420,
    },
    FnDecodeHandler::new(|result: Result<DecodedFrame<u64>, Error>| {
        match result {
            Ok(DecodedFrame::I420 { frame, user_data }) => {
                let y = frame.y_plane();
                let u = frame.u_plane();
                let v = frame.v_plane();
                println!("{}x{} user_data={}", frame.width(), frame.height(), user_data);
                // ストライドに注意 (frame.y_stride() != frame.width() のことがある)
            }
            Ok(DecodedFrame::Nv12 { frame, user_data }) => {
                let y = frame.y_plane();
                let uv = frame.uv_plane();
                println!("{}x{} user_data={}", frame.width(), frame.height(), user_data);
            }
            Err(e) => eprintln!("decode callback error: {e}"),
        }
    }),
)?;

// AVCC フォーマットのデータを非同期デコード
decoder.decode(&avcc_data, 42)?;
decoder.finish()?;
```

### コールバック結果をメインスレッドで受け取る

`EncodeHandler` / `DecodeHandler` は Video Toolbox のコールバックスレッドから呼ばれる。`mpsc::channel` で main スレッドへ受け流すのが定石。

```rust
use std::sync::mpsc;
use shiguredo_video_toolbox::{EncodedFrame, FnEncodeHandler, Error};

type EncodedResult = Result<EncodedFrame<u64>, Error>;
let (tx, rx) = mpsc::channel::<EncodedResult>();

let handler = FnEncodeHandler::new(move |result: EncodedResult| {
    if tx.send(result).is_err() {
        eprintln!("encoded results receiver is dropped");
    }
});

// encode 呼び出しの合間に rx.try_iter() で取り出して書き出す
for result in rx.try_iter() {
    let encoded = result?;
    // MP4 mux など
}
```

### VP9 / AV1 デコードの環境依存

`Vp9 { width, height }` / `Av1 { width, height }` での初期化失敗には 2 種類ある。

- 環境がそのコーデックに対応していない → `Error::UnsupportedCodec { codec }`
- `width` / `height` が 0、もしくは `i32::MAX` を超える → `Error::InvalidConfig { field, .. }`

```rust
use shiguredo_video_toolbox::{
    Decoder, DecoderCodec, DecoderConfig, Error, FnDecodeHandler, PixelFormat,
};

match Decoder::<()>::new(
    DecoderConfig {
        codec: DecoderCodec::Vp9 { width: 1920, height: 1080 },
        pixel_format: PixelFormat::I420,
    },
    FnDecodeHandler::new(|_| {}),
) {
    Ok(_decoder) => { /* デコード処理 */ }
    Err(Error::UnsupportedCodec { codec }) => {
        eprintln!("{codec} is not supported on this platform");
    }
    Err(Error::InvalidConfig { field, .. }) => {
        eprintln!("invalid decoder config: {field}");
    }
    Err(e) => return Err(e),
}
```

実行環境で本当に対応しているかを事前に判定したい場合は `supported_codecs()` を使う。

```rust
use shiguredo_video_toolbox::{supported_codecs, VideoCodecType};

let vp9_supported = supported_codecs()
    .iter()
    .find(|info| info.codec == VideoCodecType::Vp9)
    .map(|info| info.decoding.supported)
    .unwrap_or(false);
```

### 動的解像度変更

#### エンコーダー

`reconfigure()` は **常にセッションを破棄して再作成**する。未出力フレームは内部で `finish()` 経由でフラッシュされ、エンコード完了コールバックで通知される。

```rust
let new_config = EncoderConfig {
    width: 1280,
    height: 720,
    // ... 既存と同じフィールドを埋める (Default なし)
    ..
};
encoder.reconfigure(new_config)?;
```

#### デコーダー

`update_format()` は `VTDecompressionSessionCanAcceptFormatDescription()` で既存セッションが新しい `CMVideoFormatDescription` を受け入れ可能か判定し、**可能な場合はセッションを流用、不可能な場合のみ再作成**する。

```rust
// H.264: SPS/PPS が更新された場合
decoder.update_format(DecoderCodec::H264 {
    sps: &new_sps,
    pps: &new_pps,
    nalu_len_bytes: 4,
})?;

// VP9: 解像度が変更された場合
decoder.update_format(DecoderCodec::Vp9 { width: 1280, height: 720 })?;
```

| | エンコーダー | デコーダー |
|---|---|---|
| メソッド | `reconfigure(EncoderConfig)` | `update_format(DecoderCodec)` |
| 仕組み | 常にセッション破棄 + 再作成 | 受け入れ可否判定 → 必要時のみ再作成 |
| 引数 | 全エンコーダー設定 | コーデック + パラメータセットのみ |

### ゼロコピーで `CVPixelBuffer` を直接エンコード

`encode_pixel_buffer` は `video-device-rs` の `PixelBuffer::as_ptr()` などから得た `CVPixelBufferRef` を直接受け取り、データコピーを省略する。`unsafe` 関数なので呼び出し前提を満たすこと。

```rust
unsafe {
    encoder.encode_pixel_buffer(
        pixel_buffer_ptr,
        &EncodeOptions::default(),
        user_data,
    )?;
}
```

呼び出し側の責務:

- `pixel_buffer_ptr` が有効な `CVPixelBuffer` であること
- `EncoderConfig` の `width` / `height` / `pixel_format` と整合していること (寸法不一致は即クラッシュしないがエンコード結果が不正になりうる)
- 呼び出し前に `CVPixelBufferLockBaseAddress` を解除しておくこと (本関数はロック / アンロックを行わない)

内部で `CFRetain` するため、呼び出し元はこの関数の後にポインタ元を drop して構わない。ピクセルフォーマット (`y420` / `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`) のみは検証され、不一致時は `Error::PixelFormatMismatch`。

## 重要な実装メモ

### コールバック受信スレッド

- `EncodeHandler::on_encoded` / `DecodeHandler::on_decoded` は Video Toolbox 内部スレッドから呼ばれる
- ハンドラは `Send + 'static` で、ヒープに `Box<H>` として保持される (生存期間中アドレス不変)
- `Encoder` / `Decoder` 自体も `Send` (Video Toolbox セッションはスレッドセーフ)

### キーフレーム時のパラメータセット

`EncodedFrame::sps_list` / `pps_list` / `vps_list` は **キーフレーム時のみ非空**になる。
非キーフレームでは 3 つとも空 `Vec` が返る。MP4 ヘッダー (avcC / hvcC) の構築はキーフレーム到達時に行うこと。

NAL ユニット長プレフィックスは **4 バイト固定**でエンコードされる (`nalu_header_length != 4` は内部でログ + 破棄)。

### ストライド対応

`I420Frame::y_plane()` 等のスライス長は `height * y_stride()`。`y_stride() != width()` のことが普通にあるため、ストライド単位で行をコピーすること。

```rust
let stride = frame.y_stride();
let width = frame.width();
for row in 0..frame.height() {
    let row_start = row * stride;
    let row = &frame.y_plane()[row_start..row_start + width];
    // ...
}
```

`I420Frame` / `Nv12Frame` の `*_plane()` は **NULL ベースアドレスや乗算オーバーフロー時に空スライスを返す**。これは「ピクセルが無いデコード成功」ではなく **異常時のセンチネル**。`is_empty()` で判定して通常処理に進まないこと。

### 入力フレームデータの最低サイズ

`FrameData::I420 { y, u, v }` を `encode()` に渡すとき、各スライスの長さは以下を満たすこと:

- `y.len() >= width * height`
- `u.len() >= ceil(width/2) * ceil(height/2)`
- `v.len() >= ceil(width/2) * ceil(height/2)`

`FrameData::Nv12 { y, uv }`:

- `y.len() >= width * height`
- `uv.len() >= width * ceil(height/2)`

不足時は `Error::InsufficientFrameData { plane, expected, actual }`。`y` のストライドは入力幅と等しい前提 (CVPixelBuffer 内部ストライドが大きい場合は行ごとにコピーされる)。

### 検証される設定値

`EncoderConfig` の `validate_config` で以下を拒否する:

- `width == 0` / `height == 0` / `width > i32::MAX` / `height > i32::MAX`
- `fps_numerator == 0` / `fps_denominator == 0` / `fps_numerator > i32::MAX`
- `average_bitrate > i64::MAX as u64`

`DecoderCodec::Vp9` / `Av1` は `width` / `height` の同じ範囲チェックを通る。

### 防御的上限

- パラメータセット 1 個あたり: 65535 バイト (`u16::MAX`、ISO/IEC 14496-15 の `unsigned int(16)` 由来)
- エンコード出力 1 フレーム: 256 MB (`CMBlockBufferGetDataLength` 異常時の OOM 防止)

超過時はログを出力してフレームを破棄する。クランプはしない (ビットストリームを壊すため)。

### PTS

`Encoder` の内部入力 PTS は `next_input_pts` で `checked_add(fps_denominator as i64)` で更新する。オーバーフロー時は `Error::LimitExceeded`。

## サポート対応表

### エンコード

| コーデック | `CodecConfig` |
|------------|---------------|
| H.264 | `CodecConfig::H264(H264EncoderConfig)` |
| H.265 | `CodecConfig::Hevc(HevcEncoderConfig)` |

### デコード

| コーデック | `DecoderCodec` | パラメータ |
|------------|----------------|-----------|
| H.264 | `DecoderCodec::H264 { .. }` | `sps`, `pps`, `nalu_len_bytes` |
| H.265 | `DecoderCodec::Hevc { .. }` | `vps`, `sps`, `pps`, `nalu_len_bytes` |
| VP9 | `DecoderCodec::Vp9 { .. }` | `width`, `height` |
| AV1 | `DecoderCodec::Av1 { .. }` | `width`, `height` |

VP9 / AV1 はハードウェアサポートに依存するため、環境によっては利用不可。

## 依存

ランタイム依存は `log` のみ。ビルド時に `bindgen` で Apple SDK のヘッダーから FFI を生成する。

```toml
[dependencies]
log = "0.4"

[build-dependencies]
bindgen = "0.72"
```

## テストとサンプル

- `examples/raden_to_mp4.rs`: raden で描画したアニメーションを H.264 / H.265 でエンコードし MP4 に書き出す (mpsc でコールバック結果を main スレッドに受け流すパターンの参考)
- `tests/test_encoder.rs` / `tests/test_decoder.rs`: 単体テスト (セルフホストランナーで実行、Intel Mac や古い macOS では失敗しうる)
- `tests/test_codec_info.rs`: `supported_codecs()` のテスト
