//! `src/encoder.rs` に対応する単体テスト

use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, Encoder, EncoderConfig, Error, FrameData, H264EncoderConfig,
    H264EntropyMode, H264Profile, HevcEncoderConfig, HevcProfile, PixelFormat,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 480;
const SIZE: usize = WIDTH as usize * HEIGHT as usize;

fn minimal_encoder_config() -> EncoderConfig {
    EncoderConfig {
        width: 640,
        height: 480,
        codec: CodecConfig::H264(H264EncoderConfig {
            profile: H264Profile::Main,
            entropy_mode: H264EntropyMode::Cabac,
        }),
        pixel_format: PixelFormat::I420,
        average_bitrate: None,
        fps_numerator: 1,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: false,
        real_time: false,
        maximize_power_efficiency: false,
        allow_frame_reordering: false,
        allow_temporal_compression: true,
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
    }
}

fn minimal_nv12_encoder_config() -> EncoderConfig {
    let mut c = minimal_encoder_config();
    c.pixel_format = PixelFormat::Nv12;
    c
}

fn encoder_config(is_h265: bool) -> EncoderConfig {
    let codec = if is_h265 {
        CodecConfig::Hevc(HevcEncoderConfig {
            profile: HevcProfile::Main,
            allow_open_gop: true,
        })
    } else {
        CodecConfig::H264(H264EncoderConfig {
            profile: H264Profile::Main,
            entropy_mode: H264EntropyMode::Cabac,
        })
    };
    EncoderConfig {
        width: WIDTH,
        height: HEIGHT,
        codec,
        pixel_format: PixelFormat::I420,
        average_bitrate: Some(100_000),
        fps_numerator: 1,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: false,
        real_time: false,
        maximize_power_efficiency: false,
        allow_frame_reordering: false,
        allow_temporal_compression: true,
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
    }
}

/// 黒フレーム 1 枚のエンコード〜`next_frame` 取出し（`Encoder::new` の成功も含む）
///
/// [NOTE]: `encode(&[0; SIZE], ..)` のようにリテラル配列を直接渡すとコンパイルエラーになる
fn encode_black_frame_roundtrip(is_h265: bool) -> Result<(), Error> {
    let config = encoder_config(is_h265);
    let mut encoder = Encoder::new(config)?;
    let mut count = 0;

    let y = [0; SIZE];
    let u = [0; SIZE / 4];
    let v = [0; SIZE / 4];
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
    )?;

    while encoder.next_frame()?.is_some() {
        count += 1;
    }

    encoder.finish()?;
    while encoder.next_frame()?.is_some() {
        count += 1;
    }

    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn encode_h264_black() -> Result<(), Error> {
    encode_black_frame_roundtrip(false)
}

#[test]
fn encode_h265_black() -> Result<(), Error> {
    encode_black_frame_roundtrip(true)
}

#[test]
fn encoder_rejects_zero_width() {
    let mut c = minimal_encoder_config();
    c.width = 0;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_zero_height() {
    let mut c = minimal_encoder_config();
    c.height = 0;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "height",
            ..
        })
    ));
}

#[test]
fn encoder_rejects_fps_numerator_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            ..
        })
    ));
}

#[test]
fn encoder_rejects_width_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.width = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_height_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.height = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "height",
            ..
        })
    ));
}

#[test]
fn encoder_rejects_average_bitrate_above_i64_max() {
    let mut c = minimal_encoder_config();
    c.average_bitrate = Some(i64::MAX as u64 + 1);
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            ..
        })
    ));
}

#[test]
fn encoder_rejects_zero_fps_denominator() {
    let mut c = minimal_encoder_config();
    c.fps_denominator = 0;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "fps_denominator",
            ..
        })
    ));
}

#[test]
fn encoder_rejects_zero_fps_numerator() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = 0;
    assert!(matches!(
        Encoder::new(c),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero"
        })
    ));
}

#[test]
fn encode_rejects_insufficient_i420_y_plane() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_encoder_config())?;
    let y = [0u8; 1];
    let u = [0u8; 160_000];
    let v = [0u8; 160_000];
    let r = enc.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "Y", .. })
    ));
    Ok(())
}

#[test]
fn encode_rejects_insufficient_i420_u_plane() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_encoder_config())?;
    let y = vec![0u8; 640 * 480];
    let u = [0u8; 1];
    let v = vec![0u8; 160 * 120];
    let r = enc.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "U", .. })
    ));
    Ok(())
}

#[test]
fn encode_rejects_pixel_format_mismatch_i420_encoder_with_nv12_frame() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_encoder_config())?;
    let y = vec![0u8; 640 * 480];
    let uv = vec![0u8; 640 * 240];
    let r = enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
    );
    assert!(matches!(
        r,
        Err(Error::PixelFormatMismatch {
            expected: PixelFormat::I420,
            actual: PixelFormat::Nv12,
        })
    ));
    Ok(())
}

#[test]
fn encode_rejects_insufficient_nv12_uv_plane() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_nv12_encoder_config())?;
    let y = vec![0u8; 640 * 480];
    let uv = [0u8; 1];
    let r = enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "UV", .. })
    ));
    Ok(())
}
