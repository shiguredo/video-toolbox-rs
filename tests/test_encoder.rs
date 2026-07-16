//! `src/encoder.rs` に対応する単体テスト

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use shiguredo_video_toolbox::{
    CodecConfig, DataRateLimit, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error,
    FnEncodeHandler, FrameData, H264EncoderConfig, H264EntropyMode, H264Profile, HevcEncoderConfig,
    HevcProfile, PixelFormat, ReconfigureParams,
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
        data_rate_limits: Vec::new(),
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
        data_rate_limits: Vec::new(),
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
        data_rate_limits: None,
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
            data_rate_limits: None,
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
            data_rate_limits: None,
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
            data_rate_limits: None,
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
            data_rate_limits: None,
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

#[test]
fn reconfigure_updates_data_rate_limits() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let limits = vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    }];
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: None,
        data_rate_limits: Some(limits.clone()),
    })?;
    assert_eq!(encoder.config().data_rate_limits, limits);

    // 空 Vec で上限を解除できる
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: None,
        data_rate_limits: Some(Vec::new()),
    })?;
    assert!(encoder.config().data_rate_limits.is_empty());
    Ok(())
}

#[test]
fn reconfigure_rejects_more_than_two_data_rate_limits() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let limit = DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    };
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: None,
            expected_frame_rate: None,
            data_rate_limits: Some(vec![limit; 3]),
        })
        .expect_err("three data rate limits must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "data_rate_limits",
            ..
        }
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_bytes_data_rate_limit() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: None,
            expected_frame_rate: None,
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 0,
                window: Duration::from_secs(1),
            }]),
        })
        .expect_err("zero bytes data rate limit must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "data_rate_limits",
            ..
        }
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_window_data_rate_limit() -> Result<(), Error> {
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )?;
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: None,
            expected_frame_rate: None,
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 93_750,
                window: Duration::ZERO,
            }]),
        })
        .expect_err("zero window data rate limit must be rejected");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "data_rate_limits",
            ..
        }
    ));
    Ok(())
}

#[test]
fn new_rejects_invalid_data_rate_limits() {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![DataRateLimit {
        bytes: 0,
        window: Duration::from_secs(1),
    }];
    let err = Encoder::new(
        config,
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}),
    )
    .map(|_| ())
    .expect_err("invalid data rate limits must be rejected at construction");
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: "data_rate_limits",
            ..
        }
    ));
}

/// スクロールするグラデーションと下部ノイズ帯で構成された合成フレームを生成する
///
/// グラデーション部は圧縮が効き、ノイズ帯 (下部 1/4) がビット消費を押し上げるため、
/// レート制御が実際に働く負荷を安定して作れる。フレーム全面を純粋なノイズにすると
/// 最低品質でも上限バイト数を物理的に下回れず、レートリミットの検証にならない。
fn synthetic_i420_frame(frame_index: usize, seed: &mut u64) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = WIDTH as usize;
    let h = HEIGHT as usize;
    let mut y_plane = vec![0u8; SIZE];
    for row in 0..h {
        for col in 0..w {
            y_plane[row * w + col] = ((col * 255 / w + frame_index * 3) % 256) as u8;
        }
    }
    for b in y_plane[SIZE * 3 / 4..].iter_mut() {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *b = (*seed >> 33) as u8;
    }
    (y_plane, vec![128u8; SIZE / 4], vec![128u8; SIZE / 4])
}

/// DataRateLimits がウィンドウあたりの出力バイト数を実際に抑えることを検証する
///
/// - `average_bitrate` (2 Mbps) をハード上限 (750 kbps) より高く設定し、
///   リミット無しなら確実に超過する負荷 (合成フレーム) を 30 fps の PTS で
///   90 フレーム (デコード時間 3 秒ぶん) エンコードする
/// - 1 秒ウィンドウ (30 フレーム) ごとの合計バイト数が上限 + 15% 余裕に収まることを確認する
/// - エンコーダーがレート制御で崩壊してスループットが出なくなっていないことも確認する
fn data_rate_limits_cap_windowed_output(is_h265: bool) -> Result<(), Error> {
    const FRAMES: usize = 90;
    const FPS: usize = 30;
    const LIMIT_BYTES_PER_SEC: u64 = 93_750; // 750 kbps

    let mut config = encoder_config(is_h265);
    config.average_bitrate = Some(2_000_000);
    config.fps_numerator = FPS as u32;
    config.fps_denominator = 1;
    config.real_time = true;
    config.prioritize_encoding_speed_over_quality = true;
    config.data_rate_limits = vec![DataRateLimit {
        bytes: LIMIT_BYTES_PER_SEC,
        window: Duration::from_secs(1),
    }];

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

    let mut seed = 0x5eed_5eed_5eed_5eedu64;
    let encode_started = Instant::now();
    for i in 0..FRAMES {
        let (y, u, v) = synthetic_i420_frame(i, &mut seed);
        encoder.encode(
            &FrameData::I420 {
                y: &y,
                u: &u,
                v: &v,
            },
            &EncodeOptions::default(),
            i as u64,
        )?;
    }
    encoder.finish()?;
    let encode_elapsed = encode_started.elapsed();

    let callbacks = wait_and_take_results(&results, FRAMES);
    let mut sizes = Vec::new();
    for callback in callbacks {
        match callback {
            Ok(frame) => sizes.push(frame.data.len() as u64),
            Err(e) => panic!("unexpected encode callback error: {e}"),
        }
    }
    assert_eq!(sizes.len(), FRAMES);

    // 1 秒ウィンドウ (30 フレーム) ごとの合計がハード上限 + 15% 余裕に収まること。
    // 先頭ウィンドウはキーフレームとレート制御の立ち上がりを含むため対象から外す。
    let mut window_bytes = Vec::new();
    for chunk in sizes.chunks(FPS) {
        window_bytes.push(chunk.iter().sum::<u64>());
    }
    for (i, bytes) in window_bytes.iter().enumerate().skip(1) {
        assert!(
            *bytes <= LIMIT_BYTES_PER_SEC * 115 / 100,
            "window {i} produced {bytes} bytes, exceeding hard limit {LIMIT_BYTES_PER_SEC} (+15%)"
        );
    }
    // レート制御で出力が崩壊していないこと (2 Mbps 要求 + ノイズ帯なら上限の 1/4 は確実に使う)
    let total: u64 = sizes.iter().sum();
    assert!(
        total >= LIMIT_BYTES_PER_SEC * (FRAMES as u64 / FPS as u64) / 4,
        "encoder output collapsed: total {total} bytes over {FRAMES} frames"
    );
    // ハードウェアエンコードのスループットが出ていること (90 フレームを 9 秒以内 = 10 fps 以上)
    assert!(
        encode_elapsed < Duration::from_secs(9),
        "encoding {FRAMES} frames took {encode_elapsed:?}"
    );
    Ok(())
}

#[test]
fn data_rate_limits_cap_windowed_output_h264() -> Result<(), Error> {
    data_rate_limits_cap_windowed_output(false)
}

#[test]
fn data_rate_limits_cap_windowed_output_h265() -> Result<(), Error> {
    data_rate_limits_cap_windowed_output(true)
}
