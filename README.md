# video-toolbox-rs

[![crates.io](https://img.shields.io/crates/v/shiguredo_video_toolbox.svg)](https://crates.io/crates/shiguredo_video_toolbox)
[![docs.rs](https://docs.rs/shiguredo_video_toolbox/badge.svg)](https://docs.rs/shiguredo_video_toolbox)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub Actions](https://github.com/shiguredo/video-toolbox-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shiguredo/video-toolbox-rs/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/shiguredo)

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## shiguredo_video_toolbox について

Apple の [Video Toolbox](https://developer.apple.com/documentation/videotoolbox) を利用したハードウェアビデオエンコーダーおよびデコーダーの Rust バインディングです。

macOS 専用で、ビルド時に Xcode の SDK ヘッダーを参照して bindgen でバインディングを自動生成します。

## 特徴

- Video Toolbox によるハードウェアエンコード (H.264 / H.265)
- Video Toolbox によるハードウェアデコード (H.264 / H.265 / VP9 / AV1)
- コーデック固有設定を型安全に分離 (`CodecConfig` / `DecoderCodec` enum)
- ピクセルフォーマット選択 (`PixelFormat::I420` / `PixelFormat::Nv12`)
  - エンコーダー入力: `EncoderConfig` の `pixel_format` で指定
  - デコーダー出力: `DecoderConfig` の `pixel_format` で指定
- 動的解像度変更
  - エンコーダー: `Encoder::reconfigure()` でセッションを再作成
  - デコーダー: `Decoder::update_format()` でフォーマットを更新
- AVCC 形式の入出力

## 動作要件

- macOS (arm64)
- Xcode Command Line Tools (ビルド時に Video Toolbox のヘッダーファイルが必要)

## テスト

- CI（`.github/workflows/ci.yml` の `test-video-toolbox`）は **セルフホストランナー**（`labels: self-hosted, macOS, ARM64`）上で `cargo test` を実行する。
- 上記の動作要件と揃えた環境では、`supported_codecs` 等のテストが **H.264 / HEVC のハードウェア対応**を前提にしている。
- **Intel Mac**、古い macOS、仮想化・特殊構成でローカル実行した場合、同じテストが **失敗**することがある。

## ビルド

```bash
cargo build
```

### docs.rs 向けビルド

Xcode がない環境 (Linux 等) では、docs.rs 向けのドキュメント生成のみ可能です。

```bash
DOCS_RS=1 cargo doc --no-deps
```

## 使い方

### エンコード

```rust
use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error, FrameData,
    H264EncoderConfig, H264EntropyMode, H264Profile, PixelFormat,
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

let mut encoder = Encoder::new(config, |result: Result<EncodedFrame<u64>, Error>| {
    match result {
        Ok(encoded) => {
            println!(
                "encoded bytes: {} (user_data={})",
                encoded.data.len(),
                encoded.user_data
            );
        }
        Err(e) => {
            eprintln!("encode callback error: {e}");
        }
    }
})?;

// I420 フレームデータをエンコード
let frame = FrameData::I420 { y: &y_plane, u: &u_plane, v: &v_plane };
encoder.encode(&frame, &EncodeOptions::default(), 0)?;

// キーフレームを強制的に生成する
encoder.encode(&frame, &EncodeOptions {
    force_key_frame: true,
}, 1)?;

// 残りのフレームをフラッシュ
encoder.finish()?;
```

### デコード

```rust
use shiguredo_video_toolbox::{Decoder, DecoderCodec, DecoderConfig, DecodedFrame, PixelFormat};

// H.264 デコーダー (SPS / PPS が必要)
let mut decoder = Decoder::new(DecoderConfig {
    codec: DecoderCodec::H264 {
        sps: &sps,
        pps: &pps,
        nalu_len_bytes: 4,
    },
    pixel_format: PixelFormat::I420,
})?;

// AVCC フォーマットのデータをデコード
if let Some(frame) = decoder.decode(&avcc_data)? {
    match frame {
        DecodedFrame::I420(f) => {
            let y = f.y_plane();
            let u = f.u_plane();
            let v = f.v_plane();
            println!("{}x{}", f.width(), f.height());
        }
        DecodedFrame::Nv12(f) => {
            let y = f.y_plane();
            let uv = f.uv_plane();
            println!("{}x{}", f.width(), f.height());
        }
    }
}
```

## 設定

### `EncoderConfig`

エンコーダーの初期化に使用する設定です。入力ピクセルフォーマットは `pixel_format` で指定します。

| フィールド | 型 | 説明 |
|---|---|---|
| `width` | `u32` | 映像の幅 |
| `height` | `u32` | 映像の高さ |
| `codec` | `CodecConfig` | コーデック種別と固有設定 |
| `pixel_format` | `PixelFormat` | 入力ピクセルフォーマット (`I420` / `Nv12`) |
| `average_bitrate` | `Option<u64>` | 平均ビットレート (bps)、`None` でバックエンド依存 |
| `fps_numerator` | `u32` | フレームレートの分子 |
| `fps_denominator` | `u32` | フレームレートの分母 |
| `prioritize_encoding_speed_over_quality` | `bool` | 品質より速度を優先 |
| `real_time` | `bool` | リアルタイムエンコード |
| `maximize_power_efficiency` | `bool` | 電力効率最大化 |
| `allow_frame_reordering` | `bool` | フレーム再順序付け許可 |
| `allow_temporal_compression` | `bool` | 時間的圧縮許可 |
| `max_key_frame_interval` | `Option<NonZeroU32>` | 最大キーフレーム間隔 (フレーム数) |
| `max_key_frame_interval_duration` | `Option<Duration>` | 最大キーフレーム間隔 (秒数) |
| `max_frame_delay_count` | `Option<NonZeroU32>` | フレーム遅延制限 |

### `DecoderConfig`

デコーダーの初期化に使用する設定です。出力ピクセルフォーマットを `pixel_format` で指定します。

| フィールド | 型 | 説明 |
|---|---|---|
| `codec` | `DecoderCodec` | コーデック種別と初期化パラメータ |
| `pixel_format` | `PixelFormat` | 出力ピクセルフォーマット (`I420` / `Nv12`) |

### `PixelFormat`

| バリアント | 説明 |
|---|---|
| `I420` | 3 プレーン (Y, U, V) |
| `Nv12` | 2 プレーン (Y, UV interleaved) |

### `FrameData`

エンコーダーに渡す入力フレームデータです。バリアントがピクセルフォーマットを決定します。

| バリアント | フィールド | 説明 |
|---|---|---|
| `FrameData::I420` | `y`, `u`, `v` | I420 形式 (3 プレーン) |
| `FrameData::Nv12` | `y`, `uv` | NV12 形式 (2 プレーン) |

## コーデック情報の取得

`supported_codecs()` で、実行環境で利用可能なコーデック情報を一覧取得できます。

デコード判定に `VTIsHardwareDecodeSupported`、エンコード判定に `VTCopyVideoEncoderList` と `VTCopySupportedPropertyDictionaryForEncoder` を使用しています。

```rust
use shiguredo_video_toolbox::{supported_codecs, VideoCodecType, EncodingProfiles};

for info in supported_codecs() {
    println!("{:?}: decoding={}, encoding={}",
        info.codec, info.decoding.supported, info.encoding.supported);

    if info.decoding.supported {
        println!("  decoding: hw={}", info.decoding.hardware_accelerated);
    }

    if info.encoding.supported {
        println!("  encoding: hw={}", info.encoding.hardware_accelerated);
        match &info.encoding.profiles {
            EncodingProfiles::H264(profiles) => println!("  profiles: {:?}", profiles),
            EncodingProfiles::Hevc(profiles) => println!("  profiles: {:?}", profiles),
            EncodingProfiles::None => {}
        }
    }
}
```

## サポートコーデック

### エンコード

| コーデック | `CodecConfig` |
|---|---|
| H.264 | `CodecConfig::H264(H264EncoderConfig)` |
| H.265 | `CodecConfig::Hevc(HevcEncoderConfig)` |

### デコード

| コーデック | `DecoderCodec` |
|---|---|
| H.264 | `DecoderCodec::H264 { sps, pps, nalu_len_bytes }` |
| H.265 | `DecoderCodec::Hevc { vps, sps, pps, nalu_len_bytes }` |
| VP9 | `DecoderCodec::Vp9 { width, height }` |
| AV1 | `DecoderCodec::Av1 { width, height }` |

VP9 と AV1 はハードウェアサポートに依存するため、環境によっては利用できない場合があります。

デコード初期化時のエラーは次のように分かれます。

- **環境が VP9 / AV1 デコードに対応していない**など、Video Toolbox が失敗した場合は `Error::UnsupportedCodec` が返されます。
- **`width` / `height` が無効**な場合（0 である、または `i32::MAX` を超える等）は `Error::InvalidConfig` が返されます。

```rust
use shiguredo_video_toolbox::{Decoder, DecoderCodec, DecoderConfig, Error, PixelFormat};

match Decoder::new(DecoderConfig {
    codec: DecoderCodec::Vp9 { width: 1920, height: 1080 },
    pixel_format: PixelFormat::I420,
}) {
    Ok(decoder) => { /* デコード処理 */ }
    Err(Error::UnsupportedCodec { codec }) => {
        eprintln!("{codec} is not supported on this platform");
    }
    Err(Error::InvalidConfig { field, .. }) => {
        eprintln!("invalid decoder config: {field}");
    }
    Err(e) => return Err(e),
}
```

## 動的解像度変更

WebRTC やアダプティブビットレートストリーミングなど、ストリーム中に解像度が変わるユースケースに対応しています。

### エンコーダー

`reconfigure()` でセッションを再作成して解像度やその他の設定を変更できます。

Video Toolbox のエンコーダーはセッション作成時に解像度を固定するため、変更時は常にセッションの破棄と再作成が行われます。未出力フレームは自動的にフラッシュされ、エンコード完了コールバックで通知されます。

```rust
// 動的に解像度を変更
// 未出力フレームは自動的にフラッシュされる
let new_config = EncoderConfig {
    width: 1280,
    height: 720,
    codec: CodecConfig::H264(H264EncoderConfig {
        profile: H264Profile::Main,
        entropy_mode: H264EntropyMode::Cabac,
    }),
    pixel_format: PixelFormat::I420,
    average_bitrate: Some(2_000_000),
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
encoder.reconfigure(new_config)?;
```

### デコーダー

`update_format()` で新しいパラメータセットや解像度を渡してフォーマットを更新できます。H.264 / H.265 / VP9 / AV1 すべてのコーデックに対応しています。

Video Toolbox の `VTDecompressionSessionCanAcceptFormatDescription()` で既存セッションが新しいフォーマットを受け入れ可能か判定し、可能な場合はセッションを流用、不可能な場合のみセッションを再作成します。

```rust
// H.264: SPS/PPS が更新された場合
decoder.update_format(DecoderCodec::H264 {
    sps: &new_sps,
    pps: &new_pps,
    nalu_len_bytes: 4,
})?;

// H.265: VPS/SPS/PPS が更新された場合
decoder.update_format(DecoderCodec::Hevc {
    vps: &new_vps,
    sps: &new_sps,
    pps: &new_pps,
    nalu_len_bytes: 4,
})?;

// VP9: 解像度が変更された場合
decoder.update_format(DecoderCodec::Vp9 {
    width: 1280,
    height: 720,
})?;

// AV1: 解像度が変更された場合
decoder.update_format(DecoderCodec::Av1 {
    width: 1280,
    height: 720,
})?;
```

### まとめ

| | エンコーダー | デコーダー |
|---|---|---|
| メソッド | `reconfigure()` | `update_format()` |
| 仕組み | 常にセッション破棄 + 再作成 | セッション流用を判定し、不可能な場合のみ再作成 |
| 引数 | `EncoderConfig` (全設定) | `DecoderCodec` (パラメータセットのみ) |

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
