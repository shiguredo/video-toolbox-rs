//! `src/lib.rs` に対応する単体テスト（設定検証のエラーパス・state 遷移の検証）

use std::{cell::RefCell, rc::Rc};

use shiguredo_video_toolbox::{
    CodecConfig, DecodedFrame, Decoder, DecoderCodec, DecoderConfig, DecoderState, EncodeOptions,
    EncodedFrame, Encoder, EncoderConfig, EncoderState, Error, FrameData, H264EncoderConfig,
    H264EntropyMode, H264Profile, PixelFormat,
};

type EncoderSink = Rc<RefCell<Vec<Result<EncodedFrame, Error>>>>;
type DecoderSink = Rc<RefCell<Vec<Result<DecodedFrame, Error>>>>;

fn new_encoder_sink() -> EncoderSink {
    Rc::new(RefCell::new(Vec::new()))
}

fn new_decoder_sink() -> DecoderSink {
    Rc::new(RefCell::new(Vec::new()))
}

fn encoder_with_sink(sink: &EncoderSink) -> Encoder {
    let captured = sink.clone();
    Encoder::new(move |result| captured.borrow_mut().push(result))
}

fn decoder_with_sink(sink: &DecoderSink) -> Decoder {
    let captured = sink.clone();
    Decoder::new(move |result| captured.borrow_mut().push(result))
}

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

fn configured_encoder(sink: &EncoderSink, config: EncoderConfig) -> Encoder {
    let mut encoder = encoder_with_sink(sink);
    encoder.configure(config).expect("configure should succeed");
    encoder
}

// ---- configure が不正な config を拒否する ----

#[test]
fn configure_rejects_zero_width() {
    let mut c = minimal_encoder_config();
    c.width = 0;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
    assert_eq!(encoder.state(), EncoderState::Unconfigured);
}

#[test]
fn configure_rejects_zero_height() {
    let mut c = minimal_encoder_config();
    c.height = 0;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "height",
            ..
        })
    ));
}

#[test]
fn configure_rejects_fps_numerator_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            ..
        })
    ));
}

#[test]
fn configure_rejects_width_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.width = i32::MAX as u32 + 1;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn configure_rejects_height_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.height = i32::MAX as u32 + 1;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "height",
            ..
        })
    ));
}

#[test]
fn configure_rejects_average_bitrate_above_i64_max() {
    let mut c = minimal_encoder_config();
    c.average_bitrate = Some(i64::MAX as u64 + 1);
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            ..
        })
    ));
}

#[test]
fn decoder_vp9_configure_rejects_width_above_i32_max() {
    let sink = new_decoder_sink();
    let mut decoder = decoder_with_sink(&sink);
    let r = decoder.configure(DecoderConfig {
        codec: DecoderCodec::Vp9 {
            width: i32::MAX as u32 + 1,
            height: 480,
        },
        pixel_format: PixelFormat::I420,
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn decoder_av1_configure_rejects_height_above_i32_max() {
    let sink = new_decoder_sink();
    let mut decoder = decoder_with_sink(&sink);
    let r = decoder.configure(DecoderConfig {
        codec: DecoderCodec::Av1 {
            width: 640,
            height: i32::MAX as u32 + 1,
        },
        pixel_format: PixelFormat::I420,
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig {
            field: "height",
            ..
        })
    ));
}

#[test]
fn configure_rejects_zero_fps_denominator() {
    let mut c = minimal_encoder_config();
    c.fps_denominator = 0;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "fps_denominator",
            ..
        })
    ));
}

#[test]
fn configure_rejects_zero_fps_numerator() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = 0;
    let sink = new_encoder_sink();
    let mut encoder = encoder_with_sink(&sink);
    assert!(matches!(
        encoder.configure(c),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero"
        })
    ));
}

// ---- encode/decode の入力バリデーション ----

#[test]
fn encode_rejects_insufficient_i420_y_plane() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
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
}

#[test]
fn encode_rejects_insufficient_i420_u_plane() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
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
}

#[test]
fn encode_rejects_pixel_format_mismatch_i420_encoder_with_nv12_frame() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
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
}

#[test]
fn encode_rejects_insufficient_nv12_uv_plane() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_nv12_encoder_config());
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
}

// ---- state 遷移と InvalidState の検証 ----

#[test]
fn encoder_initial_state_is_unconfigured() {
    let sink = new_encoder_sink();
    let encoder = encoder_with_sink(&sink);
    assert_eq!(encoder.state(), EncoderState::Unconfigured);
    assert_eq!(encoder.encode_queue_size(), 0);
}

#[test]
fn encoder_encode_without_configure_returns_invalid_state() {
    let sink = new_encoder_sink();
    let mut enc = encoder_with_sink(&sink);
    let y = vec![0u8; 640 * 480];
    let u = vec![0u8; 160 * 120];
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
        Err(Error::InvalidState {
            operation: "encode",
            ..
        })
    ));
}

#[test]
fn encoder_flush_without_configure_returns_invalid_state() {
    let sink = new_encoder_sink();
    let mut enc = encoder_with_sink(&sink);
    let r = enc.flush();
    assert!(matches!(
        r,
        Err(Error::InvalidState {
            operation: "flush",
            ..
        })
    ));
}

#[test]
fn encoder_close_then_configure_returns_invalid_state() {
    let sink = new_encoder_sink();
    let mut enc = encoder_with_sink(&sink);
    enc.close().expect("close should succeed");
    assert_eq!(enc.state(), EncoderState::Closed);
    let r = enc.configure(minimal_encoder_config());
    assert!(matches!(
        r,
        Err(Error::InvalidState {
            operation: "configure",
            ..
        })
    ));
}

#[test]
fn encoder_close_is_idempotent() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
    enc.close().expect("first close should succeed");
    enc.close().expect("second close should be no-op");
    assert_eq!(enc.state(), EncoderState::Closed);
}

#[test]
fn encoder_reset_goes_back_to_unconfigured() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
    assert_eq!(enc.state(), EncoderState::Configured);
    enc.reset().expect("reset should succeed");
    assert_eq!(enc.state(), EncoderState::Unconfigured);
}

#[test]
fn encoder_reconfigure_via_second_configure() {
    let sink = new_encoder_sink();
    let mut enc = configured_encoder(&sink, minimal_encoder_config());
    // 2 回目の configure は既存セッションを破棄して Configured のまま設定を反映する
    let mut c2 = minimal_encoder_config();
    c2.width = 320;
    c2.height = 240;
    enc.configure(c2).expect("second configure should succeed");
    assert_eq!(enc.state(), EncoderState::Configured);
}

#[test]
fn decoder_initial_state_is_unconfigured() {
    let sink = new_decoder_sink();
    let decoder = decoder_with_sink(&sink);
    assert_eq!(decoder.state(), DecoderState::Unconfigured);
}

#[test]
fn decoder_decode_without_configure_returns_invalid_state() {
    let sink = new_decoder_sink();
    let mut decoder = decoder_with_sink(&sink);
    let r = decoder.decode(&[0u8; 4], 0);
    assert!(matches!(
        r,
        Err(Error::InvalidState {
            operation: "decode",
            ..
        })
    ));
}

#[test]
fn decoder_close_then_configure_returns_invalid_state() {
    let sink = new_decoder_sink();
    let mut decoder = decoder_with_sink(&sink);
    decoder.close().expect("close should succeed");
    let r = decoder.configure(DecoderConfig {
        codec: DecoderCodec::Vp9 {
            width: 640,
            height: 480,
        },
        pixel_format: PixelFormat::I420,
    });
    assert!(matches!(
        r,
        Err(Error::InvalidState {
            operation: "configure",
            ..
        })
    ));
}
