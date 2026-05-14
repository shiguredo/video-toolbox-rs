//! `src/encoder.rs` に対応する単体テスト

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error, FnEncodeHandler,
    FrameData, H264EncoderConfig, H264EntropyMode, H264Profile, HevcEncoderConfig, HevcProfile,
    PixelFormat, ReconfigureParams,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 480;
const SIZE: usize = WIDTH as usize * HEIGHT as usize;
type EncodeResult<T> = Result<EncodedFrame<T>, Error>;
type SharedEncodeResults<T> = Arc<Mutex<Vec<EncodeResult<T>>>>;

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

fn build_i420_black_frame() -> ([u8; SIZE], [u8; SIZE / 4], [u8; SIZE / 4]) {
    ([0; SIZE], [0; SIZE / 4], [0; SIZE / 4])
}

fn wait_and_take_results<T>(
    results: &SharedEncodeResults<T>,
    min_count: usize,
) -> Vec<EncodeResult<T>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if results.lock().expect("results mutex poisoned").len() >= min_count {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    let mut guard = results.lock().expect("results mutex poisoned");
    std::mem::take(&mut *guard)
}

fn encode_black_frame_roundtrip(is_h265: bool) -> Result<(), Error> {
    let config = encoder_config(is_h265);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        7,
    )?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match callbacks.into_iter().next().expect("callback missing") {
        Ok(frame) => {
            assert_eq!(frame.user_data, 7);
            assert!(!frame.data.is_empty());
        }
        Err(e) => panic!("unexpected encode callback error: {e}"),
    }

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
fn callback_keeps_user_data_per_frame() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        10,
    )?;
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        20,
    )?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 2);
    assert_eq!(callbacks.len(), 2);

    let mut user_data = callbacks
        .into_iter()
        .map(|r| match r {
            Ok(frame) => frame.user_data,
            Err(e) => panic!("unexpected encode callback error: {e}"),
        })
        .collect::<Vec<_>>();
    user_data.sort_unstable();
    assert_eq!(user_data, vec![10, 20]);

    Ok(())
}

#[test]
fn encoder_rejects_zero_width() {
    let mut c = minimal_encoder_config();
    c.width = 0;
    assert!(matches!(
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_zero_height() {
    let mut c = minimal_encoder_config();
    c.height = 0;
    assert!(matches!(
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
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
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
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
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_height_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.height = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
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
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
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
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
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
        Encoder::new(
            c,
            FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
        ),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero"
        })
    ));
}

#[test]
fn encode_rejects_insufficient_i420_y_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;
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
        1,
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "Y", .. })
    ));
    assert!(results.lock().expect("results mutex poisoned").is_empty());
    Ok(())
}

#[test]
fn encode_rejects_insufficient_i420_u_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;
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
        1,
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "U", .. })
    ));
    assert!(results.lock().expect("results mutex poisoned").is_empty());
    Ok(())
}

#[test]
fn encode_rejects_pixel_format_mismatch_i420_encoder_with_nv12_frame() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;
    let y = vec![0u8; 640 * 480];
    let uv = vec![0u8; 640 * 240];
    let r = enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
        1,
    );
    assert!(matches!(
        r,
        Err(Error::PixelFormatMismatch {
            expected: PixelFormat::I420,
            actual: PixelFormat::Nv12,
        })
    ));
    assert!(results.lock().expect("results mutex poisoned").is_empty());
    Ok(())
}

#[test]
fn reconfigure_updates_config_on_success() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        expected_frame_rate: Some(60),
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, 60);
    // ExpectedFrameRate は単一整数のため分母は 1 に正規化される
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

#[test]
fn reconfigure_is_noop_when_all_none() -> Result<(), Error> {
    let config = encoder_config(false);
    let before_bitrate = config.average_bitrate;
    let before_fps_num = config.fps_numerator;
    let before_fps_den = config.fps_denominator;
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    encoder.reconfigure(ReconfigureParams::default())?;
    assert_eq!(encoder.config().average_bitrate, before_bitrate);
    assert_eq!(encoder.config().fps_numerator, before_fps_num);
    assert_eq!(encoder.config().fps_denominator, before_fps_den);
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_bitrate() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: Some(0),
            expected_frame_rate: None,
        })
        .expect_err("zero bitrate must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must not be zero",
        }
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_expected_frame_rate() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: None,
            expected_frame_rate: Some(0),
        })
        .expect_err("zero frame rate must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must not be zero",
        }
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_expected_frame_rate_above_i32_max() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: None,
            expected_frame_rate: Some(i32::MAX as u32 + 1),
        })
        .expect_err("frame rate above i32::MAX must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "expected_frame_rate",
            ..
        }
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_bitrate_above_i64_max() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: Some(i64::MAX as u64 + 1),
            expected_frame_rate: None,
        })
        .expect_err("bitrate above i64::MAX must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "average_bitrate",
            ..
        }
    ));
    Ok(())
}

#[test]
fn encode_rejects_insufficient_nv12_uv_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_nv12_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results.lock().expect("results mutex poisoned").push(result);
            }
        }),
    )?;
    let y = vec![0u8; 640 * 480];
    let uv = [0u8; 1];
    let r = enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
        1,
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane: "UV", .. })
    ));
    assert!(results.lock().expect("results mutex poisoned").is_empty());
    Ok(())
}
