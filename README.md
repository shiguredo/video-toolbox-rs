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
- 動的設定更新
  - エンコーダー: `Encoder::reconfigure()` で動的に更新 (対象項目は「動的設定更新」節を参照)
  - デコーダー: `Decoder::update_format()` でフォーマットを更新 (解像度変更を含む)
- `Encoder::encode_pixel_buffer()` による CVPixelBuffer のゼロコピーエンコード
- 圧縮映像フレーム単位の非同期入出力
- エンコード結果からフレームの提示時刻 (`EncodedFrame::timestamp`) とピクチャータイプ (`EncodedFrame::picture_type`) を取得

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
    data_rate_limits: Vec::new(),
};

let mut encoder = Encoder::new(config, FnEncodeHandler::new(
    |result: Result<EncodedFrame<u64>, Error>| {
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
    },
))?;

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

#### CVPixelBuffer の直接エンコード

`Encoder::encode_pixel_buffer()` へ CVPixelBuffer のポインターを渡すと、フレームデータをコピーせずにエンコードできます。
この API はピクセルフォーマットだけを検証します。CVPixelBuffer の幅、高さ、プレーン構成を `EncoderConfig` と一致させ、エンコード前にアンロックすることは呼び出し側の責務です。

```rust
// pixel_buffer_ptr は有効かつアンロック済みの CVPixelBuffer ポインター
unsafe {
    encoder.encode_pixel_buffer(
        pixel_buffer_ptr,
        &EncodeOptions::default(),
        2,
    )?;
}
```

### デコード

```rust
use shiguredo_video_toolbox::{Decoder, DecoderCodec, DecoderConfig, DecodedFrame, FnDecodeHandler, PixelFormat};

// H.264 デコーダー (SPS / PPS が必要)
let mut decoder = Decoder::new(DecoderConfig {
    codec: DecoderCodec::H264 {
        sps: &sps,
        pps: &pps,
        nalu_len_bytes: 4,
    },
    pixel_format: PixelFormat::I420,
}, FnDecodeHandler::new(|result: Result<DecodedFrame<u64>, _>| {
    match result {
        Ok(DecodedFrame::I420 { frame, user_data }) => {
            let y = frame.y_plane();
            let u = frame.u_plane();
            let v = frame.v_plane();
            println!("{}x{} user_data={}", frame.width(), frame.height(), user_data);
        }
        Ok(DecodedFrame::Nv12 { frame, user_data }) => {
            let y = frame.y_plane();
            let uv = frame.uv_plane();
            println!("{}x{} user_data={}", frame.width(), frame.height(), user_data);
        }
        Err(e) => {
            eprintln!("decode callback error: {e}");
        }
    }
}))?;

// AVCC フォーマットのデータを非同期デコード
decoder.decode(&avcc_data, 42)?;
decoder.finish()?;
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
| `data_rate_limits` | `Vec<DataRateLimit>` | 短期ウィンドウごとのデータレート上限 |

### `DataRateLimit`

`kVTCompressionPropertyKey_DataRateLimits` に対応するデータレートのハードリミットです。
`bytes` で `window` の期間内に生成できる圧縮データの総バイト数を指定します。

| フィールド | 型 | 説明 |
|---|---|---|
| `bytes` | `u64` | ウィンドウあたりの総バイト数の上限 |
| `window` | `Duration` | ウィンドウの長さ |

指定できるリミットは最大 2 個です。`bytes` と `window` には 0 を指定できず、`bytes` は `i64::MAX` 以下である必要があります。
`EncoderConfig::data_rate_limits` は空の `Vec` が上限なし (未設定) を意味します。

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

### `EncodedFrame`

エンコード結果です。`EncodeHandler::on_encoded` に `Result<EncodedFrame<T>, Error>` として渡されます。

| フィールド | 型 | 説明 |
|---|---|---|
| `timestamp` | `Option<Timestamp>` | フレームの提示時刻。有効な時刻が得られなかった場合は `None` |
| `picture_type` | `PictureType` | ピクチャータイプ |
| `sps_list` | `Vec<Vec<u8>>` | SPS (キーフレームのときのみ) |
| `pps_list` | `Vec<Vec<u8>>` | PPS (キーフレームのときのみ) |
| `vps_list` | `Vec<Vec<u8>>` | VPS (H.265 のキーフレームのときのみ) |
| `data` | `Vec<u8>` | 圧縮データ (AVCC 形式) |
| `user_data` | `T` | `encode` / `encode_pixel_buffer` 呼び出し時に指定したユーザーデータ |

`timestamp` は `value / timescale` 秒を表す有理数です。`timescale` には、その時刻を生成したときに
エンコーダーが入力フレームのタイムスタンプに使っていた値 (`EncoderConfig::fps_numerator`) が入ります。
`Encoder::reconfigure` でフレームレートを変更すると、変更前のフレームと変更後のフレームで
`timescale` が異なるため、複数の `timescale` をまたいで時刻を比較する場合は `Timestamp::seconds()`
で秒に直してください。1 回のエンコードで投入したフレームのうち、フレームレートの変更をまたがない
ものは `timescale` が同じになるため、`value` の差分をそのまま表示順の間隔として使えます。

`picture_type` は Video Toolbox が返すフレーム種別の情報から判定できる範囲だけを表します。
Video Toolbox は I フレームと IDR フレームを区別しないため、`PictureType::I` には
IDR フレームも含まれます。`PictureType::B` は `allow_frame_reordering` が `true` の場合にだけ現れます。
キーフレームかどうかは `PictureType::I` かどうかで判定します。

```rust
use shiguredo_video_toolbox::{EncodedFrame, Error, PictureType};

// エンコードコールバックの中
let encoded: EncodedFrame<u64> = match result {
    Ok(encoded) => encoded,
    Err(e) => {
        eprintln!("encode callback error: {e}");
        return;
    }
};

// 提示時刻 (秒) と、その時刻の目盛り
if let Some(timestamp) = encoded.timestamp {
    println!("pts: {} (timescale={})", timestamp.seconds(), timestamp.timescale);
}
match encoded.picture_type {
    // キーフレームかどうかは PictureType::I かどうかで判定します
    PictureType::I => println!("keyframe"),
    PictureType::B => println!("b frame"),
    PictureType::P => println!("p frame"),
    PictureType::Unknown => println!("unknown picture type"),
}
```

## コーデック情報の取得

`supported_codecs()` で、解像度に依存しないコーデック情報を一覧取得できます。
デコード判定に `VTIsHardwareDecodeSupported`、エンコーダーの一覧に `VTCopyVideoEncoderList` を使用しています。

`DecodingInfo` の `hardware_accelerated` は「ハードウェアデコードが可能か」です。

```rust
use shiguredo_video_toolbox::{VideoCodecType, supported_codecs};

for info in supported_codecs() {
    println!("{:?}: decoding_hw={}, encoders={}",
        info.codec, info.decoding.hardware_accelerated, info.encoders.len());

    // ハードウェアデコードが使えるか
    println!("  decoding: hw={}", info.decoding.hardware_accelerated);

    // エンコーダー 1 件ずつの属性。hardware_accelerated はこのエントリ自身が
    // ハードウェア実装かどうかを表す
    for encoder in &info.encoders {
        println!("  encoder: id={}, hw={}",
            encoder.encoder_id, encoder.hardware_accelerated);
        println!("    encoder_name={:?}, codec_name={:?}",
            encoder.encoder_name, encoder.codec_name);
        println!("    frame_reordering={}, multi_pass={}",
            encoder.supports_frame_reordering, encoder.supports_multi_pass);

        // 同じコーデックの他のエンコーダーとの相対値。None は「不明」
        println!("    ratings: performance={:?}, quality={:?}",
            encoder.performance_rating, encoder.quality_rating);
    }
}
```

`CodecInfo::encoders` が空の場合は、そのコーデックではエンコードできません。

どのエンコーダーが選ばれるかは解像度によって変わるため、解像度が決まっている場合は `query_encoding_capabilities()` を使います。

```rust
use shiguredo_video_toolbox::{EncodingProfiles, VideoCodecType, query_encoding_capabilities};

// エンコーダーが無いコーデック (VP9 / AV1) や、解像度が不正な場合は None になります
if let Some(capabilities) = query_encoding_capabilities(VideoCodecType::H264, 1920, 1080) {
    // この解像度でハードウェアエンコーダーが使えるかは、選ばれるエンコーダーの属性で判定します
    println!("hardware_accelerated={}", capabilities.encoder.hardware_accelerated);
    println!("encoder_id={}", capabilities.encoder.encoder_id);

    match &capabilities.profiles {
        Some(EncodingProfiles::H264(profiles)) => println!("profiles: {profiles:?}"),
        Some(EncodingProfiles::Hevc(profiles)) => println!("profiles: {profiles:?}"),
        // Video Toolbox がプロファイル一覧を返さなかった場合
        None => println!("profiles: unknown"),
    }
}
```

`query_encoding_capabilities()` は `encoderSpecification` に NULL を渡した
`VTCopySupportedPropertyDictionaryForEncoder` を使用します。これは `VTCompressionSessionCreate` と
同じ既定の選択（解像度に対応するハードウェアエンコーダーがあればそれを、無ければソフトウェア
エンコーダーを選ぶ）なので、返る `encoder` は実際に使われるエンコーダーです。
ハードウェアエンコーダーの資源が枯渇している場合、この照会が成功してもセッションの生成に失敗することがあります。

`profiles` には、Video Toolbox がプロファイルレベルに指定できる値のうち、このクレートが
`H264EncodingProfile` / `HevcEncodingProfile` として表現できるものだけが入ります。
Video Toolbox は H.264 の High 4:2:2 / High 4:4:4 Predictive、HEVC の 4:4:4 系や Monochrome 系なども
返しうるため、`profiles` に含まれないことが「そのプロファイルが使えない」ことを意味するとは限りません。

解像度の上下限と、ビットレート・フレームレートなどの数値プロパティの範囲は公開していません。
解像度ごとの可否は `query_encoding_capabilities()` で判定してください。

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
use shiguredo_video_toolbox::{Decoder, DecoderCodec, DecoderConfig, Error, FnDecodeHandler, PixelFormat};

match Decoder::<()>::new(DecoderConfig {
    codec: DecoderCodec::Vp9 { width: 1920, height: 1080 },
    pixel_format: PixelFormat::I420,
}, FnDecodeHandler::new(|_| {})) {
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

## 動的設定更新

WebRTC やアダプティブビットレートストリーミングなど、ストリーム中に設定を変更するユースケースに対応しています。

### エンコーダー

`reconfigure()` で `ReconfigureParams` を渡し、動的に変更可能な項目だけを更新します。
`VTSessionSetProperties` を 1 回呼び出して指定された項目を一括反映するため、セッション再作成は行われません。

動的に更新できる項目は `average_bitrate` / `expected_frame_rate` の 2 つです。
解像度・コーデック・ピクセルフォーマットなど Video Toolbox が動的変更をサポートしない項目は、
`Encoder` を作り直して対応します。

`expected_frame_rate` を更新すると `fps_numerator` / `fps_denominator` は `expected_frame_rate / 1` に
正規化されます (分数 fps は保持されません)。分数 fps を保持したい場合は `Encoder` を作り直してください。

`Encoder::config()` は、初期化時の設定に直近の動的更新を反映した値を返します。
Video Toolbox が内部で丸めた実効値ではありません。

```rust
use shiguredo_video_toolbox::ReconfigureParams;

// ビットレートとフレームレートを動的に更新
// 未指定 (None) の項目は現在値を維持する
encoder.reconfigure(ReconfigureParams {
    average_bitrate: Some(2_000_000),
    expected_frame_rate: Some(60),
    ..Default::default()
})?;
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
| 仕組み | `VTSessionSetProperties` で動的更新 (セッション再作成なし) | セッション流用を判定し、不可能な場合のみ再作成 |
| 引数 | `ReconfigureParams` (動的更新可能項目のみ) | `DecoderCodec` (パラメータセットのみ) |
| 対応外項目 | 解像度・コーデック・ピクセルフォーマット → `Encoder` を作り直す | (`DecoderCodec` バリアントが対応するもの以外) |

## 統計値

`Encoder::stats()` / `Decoder::stats()` で、エンコード / デコードの進行状況をメトリクスとして取得できます。

統計値はエンコーダー / デコーダーを操作するスレッドと Video Toolbox のコールバックスレッドの
両方が更新するため、戻り値はエンコーダー / デコーダーと共有されている値への参照です。
複数のフィールドを読む間に値が変化し得るので、値を保存しておきたい場合は `clone()` してください。

`Counter` は単調増加の通算値、`Gauge` は増減する時点値です。どちらも `clone()` は現在値の
コピーを返します。

### `EncoderStats`

| フィールド | 型 | 説明 |
|---|---|---|
| `total_encode_count` | `Counter` | `encode()` / `encode_pixel_buffer()` が Video Toolbox に受理された通算回数 |
| `total_output_frame_count` | `Counter` | 出力コールバックに `Ok` を渡した通算回数 |
| `total_error_count` | `Counter` | 出力コールバックに `Err` を渡した通算回数 |
| `total_reconfigure_count` | `Counter` | `reconfigure()` が成功した通算回数 |
| `in_flight_frames` | `Gauge` | 送信済みでまだ出力コールバックが来ていないフレーム数の現在値 |

### `DecoderStats`

| フィールド | 型 | 説明 |
|---|---|---|
| `total_decode_count` | `Counter` | `decode()` が Video Toolbox に受理された通算回数 |
| `total_output_frame_count` | `Counter` | 出力コールバックに `Ok` を渡した通算回数 |
| `total_error_count` | `Counter` | 出力コールバックに `Err` を渡した通算回数 |
| `total_create_session_count` | `Counter` | デコーダーセッションの作成に成功した通算回数 |
| `total_update_format_count` | `Counter` | `update_format()` がセッションを流用した通算回数 |
| `total_recreate_session_count` | `Counter` | `update_format()` がセッションを再作成した通算回数 |
| `in_flight_frames` | `Gauge` | 送信済みでまだ出力コールバックが来ていないフレーム数の現在値 |

`in_flight_frames` は送信の直前に増え、出力コールバックがユーザーデータを回収した時点で減ります。
Video Toolbox が満杯を起こさない上限値は公開していないため、利用側で上限を決めて
`finish()` を挟むことで、処理中のフレーム数を制御できます。

`total_create_session_count` は `Decoder::new()` の初回作成を含みます。

```rust
// 送信済みで処理中のフレーム数を確認し、閾値に達したらフラッシュする
let in_flight = encoder.stats().in_flight_frames.get();
if in_flight >= 4 {
    encoder.finish()?;
}

// 通算値のコピーを取得する (フィールド間の一貫性は保証されない)
let stats = encoder.stats().clone();
println!(
    "encoded={} output={} error={}",
    stats.total_encode_count.get(),
    stats.total_output_frame_count.get(),
    stats.total_error_count.get()
);

// update_format() がセッションを流用したか再作成したかも統計値で確認できる
println!(
    "reuse={} recreate={}",
    decoder.stats().total_update_format_count.get(),
    decoder.stats().total_recreate_session_count.get()
);
```

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
