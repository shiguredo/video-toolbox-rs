//! `encoder` モジュール (src/encoder.rs と src/encoder/) に対応する単体テスト

mod helpers;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
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
        data_rate_limits: None,
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
        data_rate_limits: None,
    }
}

fn build_i420_black_frame() -> ([u8; SIZE], [u8; SIZE / 4], [u8; SIZE / 4]) {
    ([0; SIZE], [0; SIZE / 4], [0; SIZE / 4])
}

/// エンコード結果を無視するハンドラー (構築・設定の検証だけで完結するテスト用)
fn noop_encode_handler() -> FnEncodeHandler<()> {
    FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
}

/// 検証エラーを期待して `reconfigure` を呼び、返された `Error` を取り出す
fn reconfigure_err(params: ReconfigureParams) -> Error {
    let mut encoder = Encoder::new(encoder_config(false), noop_encode_handler())
        .expect("encoder construction must succeed");
    encoder
        .reconfigure(params)
        .expect_err("invalid params must be rejected")
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
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "width" && reason == "must not be zero"
    ));
}

#[test]
fn encoder_rejects_zero_height() {
    let mut c = minimal_encoder_config();
    c.height = 0;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "height" && reason == "must not be zero"
    ));
}

#[test]
fn encoder_rejects_fps_numerator_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "fps_numerator" && reason == "must fit in i32 for CMTime timescale"
    ));
}

#[test]
fn encoder_rejects_width_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.width = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "width" && reason == "must fit in i32 for Video Toolbox dimensions"
    ));
}

#[test]
fn encoder_rejects_height_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.height = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "height" && reason == "must fit in i32 for Video Toolbox dimensions"
    ));
}

#[test]
fn encoder_rejects_average_bitrate_above_i64_max() {
    let mut c = minimal_encoder_config();
    c.average_bitrate = Some(i64::MAX as u64 + 1);
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "average_bitrate" && reason == "must fit in i64 for CFNumber"
    ));
}

#[test]
fn encoder_rejects_zero_fps_denominator() {
    let mut c = minimal_encoder_config();
    c.fps_denominator = 0;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "fps_denominator" && reason == "must not be zero"
    ));
}

#[test]
fn encoder_rejects_zero_fps_numerator() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = 0;
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "fps_numerator" && reason == "must not be zero"
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
        Err(Error::InsufficientFrameData { plane, .. }) if plane == "Y"
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
        Err(Error::InsufficientFrameData { plane, .. }) if plane == "U"
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
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        expected_frame_rate: Some(60),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, 60);
    // ExpectedFrameRate は単一整数のため分母は 1 に正規化される
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

#[test]
fn reconfigure_updates_only_average_bitrate() -> Result<(), Error> {
    // bitrate のみ更新で fps (30_000/1_001) が初期値のまま保たれることを確認する。
    // 初期 fps を分数にすることで、既定値 (1/1) への巻き戻りや分母の誤正規化を検出できる。
    // 検証対象は `self.config` への反映のみで、セッション側のプロパティ設定は対象外。
    let mut config = encoder_config(false);
    config.fps_numerator = 30_000;
    config.fps_denominator = 1_001;
    let initial_fps_num = config.fps_numerator;
    let initial_fps_den = config.fps_denominator;
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, initial_fps_num);
    assert_eq!(encoder.config().fps_denominator, initial_fps_den);
    Ok(())
}

#[test]
fn reconfigure_updates_only_expected_frame_rate() -> Result<(), Error> {
    // fps のみ更新で bitrate が初期値のまま保たれ、分母が 1 に正規化されることを確認する。
    // 初期 fps を分数にすることで、正規化漏れ (分母 1_001 のまま) を検出できる。
    // 初期 bitrate は既定値と区別し、fps 更新時に bitrate が初期値へ上書きされる回帰を検出できるようにする。
    // 検証対象は `self.config` への反映のみで、PTS の再スケールは対象外。
    let mut config = encoder_config(false);
    config.fps_numerator = 30_000;
    config.fps_denominator = 1_001;
    config.average_bitrate = Some(250_000);
    let initial_bitrate = config.average_bitrate;
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        expected_frame_rate: Some(60),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().average_bitrate, initial_bitrate);
    assert_eq!(encoder.config().fps_numerator, 60);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

#[test]
fn reconfigure_is_noop_when_all_none() -> Result<(), Error> {
    // 全項目 None の reconfigure は no-op であり、設定を変えずセッションも壊さないことを確認する
    let config = encoder_config(false);
    let before_bitrate = config.average_bitrate;
    let before_fps_num = config.fps_numerator;
    let before_fps_den = config.fps_denominator;
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
    encoder.reconfigure(ReconfigureParams::default())?;
    assert_eq!(encoder.config().average_bitrate, before_bitrate);
    assert_eq!(encoder.config().fps_numerator, before_fps_num);
    assert_eq!(encoder.config().fps_denominator, before_fps_den);
    // no-op の reconfigure がセッションを壊していないこと (後続の encode が成功すること) を確認する
    let (y, u, v) = build_i420_black_frame();
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        1,
    )?;
    encoder.finish()?;
    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match &callbacks[0] {
        Ok(frame) => {
            assert_eq!(frame.user_data, 1);
            assert!(
                !frame.data.is_empty(),
                "no-op の reconfigure 後の encode は実データを返すこと"
            );
        }
        Err(e) => panic!("no-op の reconfigure 後の encode は成功すること: {e}"),
    }
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_bitrate() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            average_bitrate: Some(0),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "average_bitrate" && reason == "must not be zero"
    ));
}

#[test]
fn reconfigure_rejects_zero_expected_frame_rate() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            expected_frame_rate: Some(0),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "expected_frame_rate" && reason == "must not be zero"
    ));
}

#[test]
fn reconfigure_rejects_expected_frame_rate_above_i32_max() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            expected_frame_rate: Some(i32::MAX as u32 + 1),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "expected_frame_rate" && reason == "must fit in i32 for CFNumber"
    ));
}

#[test]
fn reconfigure_rejects_bitrate_above_i64_max() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            average_bitrate: Some(i64::MAX as u64 + 1),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "average_bitrate" && reason == "must fit in i64 for CFNumber"
    ));
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
        Err(Error::InsufficientFrameData { plane, .. }) if plane == "UV"
    ));
    assert!(results.lock().expect("results mutex poisoned").is_empty());
    Ok(())
}

#[test]
fn reconfigure_updates_data_rate_limits() -> Result<(), Error> {
    let mut encoder = Encoder::new(encoder_config(false), noop_encode_handler())?;
    let limits = vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    }];
    encoder.reconfigure(ReconfigureParams {
        data_rate_limits: Some(limits.clone()),
        ..Default::default()
    })?;
    assert_eq!(encoder.config().data_rate_limits, Some(limits));

    // 空 Vec で上限を解除できる
    encoder.reconfigure(ReconfigureParams {
        data_rate_limits: Some(Vec::new()),
        ..Default::default()
    })?;
    assert!(encoder.config().data_rate_limits.is_none());
    Ok(())
}

#[test]
fn reconfigure_rejects_more_than_two_data_rate_limits() {
    let limit = DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    };
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            data_rate_limits: Some(vec![limit; 3]),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "must contain at most two limits"
    ));
}

#[test]
fn reconfigure_rejects_zero_bytes_data_rate_limit() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 0,
                window: Duration::from_secs(1),
            }]),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "bytes must not be zero"
    ));
}

#[test]
fn reconfigure_rejects_data_rate_limit_bytes_above_i64_max() {
    // `bytes` は CFNumber (SInt64) に変換されるため i64::MAX を超える値は拒否される
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: i64::MAX as u64 + 1,
                window: Duration::from_secs(1),
            }]),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "bytes must fit in i64 for CFNumber"
    ));
}

#[test]
fn reconfigure_rejects_zero_window_data_rate_limit() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 93_750,
                window: Duration::ZERO,
            }]),
            ..Default::default()
        }),
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "window must not be zero"
    ));
}

#[test]
fn new_rejects_invalid_data_rate_limits() {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = Some(vec![DataRateLimit {
        bytes: 0,
        window: Duration::from_secs(1),
    }]);
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("invalid data rate limits must be rejected at construction");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "bytes must not be zero"
    ));
}

#[test]
fn new_rejects_zero_average_bitrate() {
    let mut config = minimal_encoder_config();
    config.average_bitrate = Some(0);
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("zero average bitrate must be rejected at construction");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "average_bitrate" && reason == "must not be zero"
    ));
}

#[test]
fn new_normalizes_empty_data_rate_limits_to_none() -> Result<(), Error> {
    // `Some(空 Vec)` は未設定と同義なので `config()` では `None` に正規化される
    let mut config = minimal_encoder_config();
    config.data_rate_limits = Some(Vec::new());
    let encoder = Encoder::new(config, noop_encode_handler())?;
    assert!(encoder.config().data_rate_limits.is_none());
    Ok(())
}

#[test]
fn encoder_config_returns_initial_value() -> Result<(), Error> {
    let mut config = encoder_config(false);
    // 既定値と区別できる値にする (ハードコードされた既定値を返す回帰を検出するため)
    config.fps_numerator = 30;
    config.real_time = true;
    config.prioritize_encoding_speed_over_quality = true;
    config.maximize_power_efficiency = true;
    config.allow_frame_reordering = true;
    config.max_key_frame_interval = std::num::NonZeroU32::new(60);
    config.max_key_frame_interval_duration = Some(Duration::from_secs(2));
    config.max_frame_delay_count = std::num::NonZeroU32::new(2);
    config.data_rate_limits = Some(vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    }]);
    let encoder = Encoder::new(config.clone(), noop_encode_handler())?;
    let got = encoder.config();
    assert_eq!(got.width, config.width);
    assert_eq!(got.height, config.height);
    assert_eq!(got.fps_numerator, config.fps_numerator);
    assert_eq!(got.fps_denominator, config.fps_denominator);
    assert_eq!(got.average_bitrate, config.average_bitrate);
    assert_eq!(got.pixel_format, config.pixel_format);
    assert_eq!(got.real_time, config.real_time);
    assert_eq!(got.allow_frame_reordering, config.allow_frame_reordering);
    assert_eq!(
        got.allow_temporal_compression,
        config.allow_temporal_compression
    );
    assert_eq!(
        got.prioritize_encoding_speed_over_quality,
        config.prioritize_encoding_speed_over_quality
    );
    assert_eq!(
        got.maximize_power_efficiency,
        config.maximize_power_efficiency
    );
    assert_eq!(got.max_key_frame_interval, config.max_key_frame_interval);
    assert_eq!(
        got.max_key_frame_interval_duration,
        config.max_key_frame_interval_duration
    );
    assert_eq!(got.max_frame_delay_count, config.max_frame_delay_count);
    assert_eq!(got.data_rate_limits, config.data_rate_limits);
    // codec はバリアントと中身を確認する
    match (&got.codec, &config.codec) {
        (CodecConfig::H264(a), CodecConfig::H264(b)) => {
            assert_eq!(a.profile, b.profile);
            assert_eq!(a.entropy_mode, b.entropy_mode);
        }
        _ => panic!("codec variant mismatch"),
    }
    Ok(())
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
/// - 1 秒ウィンドウ (30 フレーム) ごとの合計バイト数が上限 + 50% 余裕に収まることを確認する。
///   VTCompressionProperties.h は "some codecs do not support limiting to specified data rates"
///   とレート制御の遵守を保証していないため、実装揺れを許容する広めのマージンを取る
///   (リミット無しの 2 Mbps 出力とは明確に区別できる)
/// - エンコーダーがレート制御で崩壊して出力が出なくなっていないことも確認する
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
    config.data_rate_limits = Some(vec![DataRateLimit {
        bytes: LIMIT_BYTES_PER_SEC,
        window: Duration::from_secs(1),
    }]);

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

    let callbacks = wait_and_take_results(&results, FRAMES);
    let mut sizes = Vec::new();
    for callback in callbacks {
        match callback {
            Ok(frame) => sizes.push(frame.data.len() as u64),
            Err(e) => panic!("unexpected encode callback error: {e}"),
        }
    }
    assert_eq!(sizes.len(), FRAMES);

    // 1 秒ウィンドウ (30 フレーム) ごとの合計がハード上限 + 50% 余裕に収まること。
    // 先頭ウィンドウはキーフレームとレート制御の立ち上がりを含むため対象から外す。
    let mut window_bytes = Vec::new();
    for chunk in sizes.chunks(FPS) {
        window_bytes.push(chunk.iter().sum::<u64>());
    }
    for (i, bytes) in window_bytes.iter().enumerate().skip(1) {
        assert!(
            *bytes <= LIMIT_BYTES_PER_SEC * 150 / 100,
            "window {i} produced {bytes} bytes, exceeding hard limit {LIMIT_BYTES_PER_SEC} (+50%)"
        );
    }
    // レート制御で出力が崩壊していないこと (2 Mbps 要求 + ノイズ帯なら上限の 1/4 は確実に使う)
    let total: u64 = sizes.iter().sum();
    assert!(
        total >= LIMIT_BYTES_PER_SEC * (FRAMES as u64 / FPS as u64) / 4,
        "encoder output collapsed: total {total} bytes over {FRAMES} frames"
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

/// ユーザーハンドラが 1 回目に panic してもプロセスが abort せず、
/// panic 捕捉後にセッションが継続して後続フレームが届くことを確認する
///
/// 1 回目のコールバックで panic し、2 回目 (user_data = 2) だけが届くことを期待している。
/// `allow_frame_reordering: false` のため投入順でコールバックされる (Video Toolbox の
/// 保証ではなく実測前提であり、既存テストも同じ前提)。
#[test]
fn handler_panic_is_caught_and_encode_continues() -> Result<(), Error> {
    // コールバックは Video Toolbox の別スレッドで実行されるため、グローバル subscriber で
    // ログを収集する (with_default はスレッドローカルで届かない)
    let logs = helpers::init_global_log_collector();
    helpers::clear_logs(&logs);

    encode_with_panicking_handler()?;

    let log = helpers::take_logs(&logs);
    helpers::assert_log_contains(&log, "output_callback_h264: user handler panicked");
    helpers::assert_log_contains(&log, "intentional panic in test handler");
    Ok(())
}

fn encode_with_panicking_handler() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let panicked = Arc::new(AtomicBool::new(false));
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            let panicked = Arc::clone(&panicked);
            move |result: Result<EncodedFrame<u64>, Error>| {
                // 1 回目のコールバックだけ panic して、後続は正常に結果を返す
                if !panicked.swap(true, Ordering::Relaxed) {
                    panic!("intentional panic in test handler");
                }
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
        1,
    )?;
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        2,
    )?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match &callbacks[0] {
        Ok(frame) => assert_eq!(frame.user_data, 2),
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    }
    Ok(())
}
