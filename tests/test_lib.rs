//! `src/lib.rs` に対応する単体テスト（設定検証のエラーパス等）

use shiguredo_video_toolbox::{
    CodecConfig, Encoder, EncoderConfig, Error, H264EncoderConfig, H264EntropyMode, H264Profile,
    PixelFormat,
};

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
