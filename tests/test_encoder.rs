//! `src/encoder.rs` に対応する単体テスト

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error, FrameData,
    H264EncoderConfig, H264EntropyMode, H264Profile, HevcEncoderConfig, HevcProfile, PixelFormat,
    ReconfigureParams,
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
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if results.lock().expect("results mutex poisoned").len() >= min_count {
            break;
        }
        if Instant::now() >= deadline {
            panic!("timeout waiting for {min_count} encode callbacks");
        }
        thread::sleep(Duration::from_millis(1));
    }

    let mut guard = results.lock().expect("results mutex poisoned");
    std::mem::take(&mut *guard)
}

fn encode_black_frame_roundtrip(is_h265: bool) -> Result<(), Error> {
    let config = encoder_config(is_h265);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;

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
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;

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

    // `allow_frame_reordering: false` なので、入力順がそのままコールバック順として観測される
    let user_data = callbacks
        .into_iter()
        .map(|r| match r {
            Ok(frame) => frame.user_data,
            Err(e) => panic!("unexpected encode callback error: {e}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(user_data, vec![10, 20]);

    Ok(())
}

#[test]
fn encoder_rejects_zero_width() {
    let mut c = minimal_encoder_config();
    c.width = 0;
    assert!(matches!(
        Encoder::<()>::new(c, |_| {}),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_zero_height() {
    let mut c = minimal_encoder_config();
    c.height = 0;
    assert!(matches!(
        Encoder::<()>::new(c, |_| {}),
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
        Encoder::<()>::new(c, |_| {}),
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
        Encoder::<()>::new(c, |_| {}),
        Err(Error::InvalidConfig { field: "width", .. })
    ));
}

#[test]
fn encoder_rejects_height_above_i32_max() {
    let mut c = minimal_encoder_config();
    c.height = i32::MAX as u32 + 1;
    assert!(matches!(
        Encoder::<()>::new(c, |_| {}),
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
        Encoder::<()>::new(c, |_| {}),
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must fit in i64 for CFNumber"
        })
    ));
}

#[test]
fn encoder_rejects_zero_average_bitrate() {
    let mut c = minimal_encoder_config();
    c.average_bitrate = Some(0);
    assert!(matches!(
        Encoder::<()>::new(c, |_| {}),
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must not be zero"
        })
    ));
}

#[test]
fn encoder_accepts_average_bitrate_at_i64_max() {
    let mut c = minimal_encoder_config();
    c.average_bitrate = Some(i64::MAX as u64);
    // i64::MAX ちょうどは CFNumber i64 に収まるので Encoder の構築が成功するはず
    Encoder::<()>::new(c, |_| {}).expect("encoder should accept i64::MAX bitrate");
}

#[test]
fn encoder_accepts_fps_numerator_at_i32_max() {
    let mut c = minimal_encoder_config();
    c.fps_numerator = i32::MAX as u32;
    Encoder::<()>::new(c, |_| {}).expect("encoder should accept fps_numerator = i32::MAX");
}

#[test]
fn encoder_rejects_zero_fps_denominator() {
    let mut c = minimal_encoder_config();
    c.fps_denominator = 0;
    assert!(matches!(
        Encoder::<()>::new(c, |_| {}),
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
        Encoder::<()>::new(c, |_| {}),
        Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero"
        })
    ));
}

#[test]
fn encode_rejects_insufficient_i420_y_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(minimal_encoder_config(), {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
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
    let mut enc = Encoder::new(minimal_encoder_config(), {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
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
    let mut enc = Encoder::new(minimal_encoder_config(), {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
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
fn reconfigure_no_op_when_all_none() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;

    let before_bitrate = encoder.config().average_bitrate;
    let before_fps_num = encoder.config().fps_numerator;
    let before_fps_den = encoder.config().fps_denominator;

    encoder.reconfigure(ReconfigureParams::default())?;

    // 全項目 None なら設定は変化しない
    assert_eq!(encoder.config().average_bitrate, before_bitrate);
    assert_eq!(encoder.config().fps_numerator, before_fps_num);
    assert_eq!(encoder.config().fps_denominator, before_fps_den);

    // 後続の encode が成功することを確認
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
    assert!(callbacks[0].is_ok());
    Ok(())
}

#[test]
fn reconfigure_updates_average_bitrate() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(500_000),
        expected_frame_rate: None,
    })?;

    assert_eq!(encoder.config().average_bitrate, Some(500_000));
    Ok(())
}

#[test]
fn reconfigure_updates_expected_frame_rate() -> Result<(), Error> {
    let mut config = encoder_config(false);
    // 分数 fps を初期値にしておき、reconfigure 後に分母が 1 に正規化されることを確認する
    config.fps_numerator = 30_000;
    config.fps_denominator = 1_001;
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: Some(60),
    })?;

    assert_eq!(encoder.config().fps_numerator, 60);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_expected_frame_rate() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    let r = encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: Some(0),
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must not be zero"
        })
    ));
    // 失敗時には設定が変更されていないこと
    assert_eq!(encoder.config().fps_numerator, 1);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

#[test]
fn reconfigure_rejects_expected_frame_rate_above_i32_max() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    let r = encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: Some(i32::MAX as u32 + 1),
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must fit in i32 for CFNumber"
        })
    ));
    Ok(())
}

#[test]
fn reconfigure_rejects_average_bitrate_above_i64_max() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    let before = encoder.config().average_bitrate;
    let r = encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(i64::MAX as u64 + 1),
        expected_frame_rate: None,
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must fit in i64 for CFNumber"
        })
    ));
    // 失敗時には設定が変更されていないこと
    assert_eq!(encoder.config().average_bitrate, before);
    Ok(())
}

#[test]
fn reconfigure_accepts_expected_frame_rate_at_i32_max() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: None,
        expected_frame_rate: Some(i32::MAX as u32),
    })?;
    assert_eq!(encoder.config().fps_numerator, i32::MAX as u32);
    assert_eq!(encoder.config().fps_denominator, 1);
    // reconfigure 後でも encode→出力コールバックまで通ることを確認する
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
    assert!(callbacks.iter().all(|r| r.is_ok()));
    Ok(())
}

#[test]
fn reconfigure_accepts_average_bitrate_at_i64_max() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(i64::MAX as u64),
        expected_frame_rate: None,
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(i64::MAX as u64));
    // reconfigure 後でも encode→出力コールバックまで通ることを確認する
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
    assert!(callbacks.iter().all(|r| r.is_ok()));
    Ok(())
}

#[test]
fn reconfigure_rejects_zero_average_bitrate() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::<()>::new(config, |_| {})?;

    let before = encoder.config().average_bitrate;
    let r = encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(0),
        expected_frame_rate: None,
    });
    assert!(matches!(
        r,
        Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must not be zero"
        })
    ));
    // 失敗時には設定が変更されていないこと
    assert_eq!(encoder.config().average_bitrate, before);
    Ok(())
}

#[test]
fn reconfigure_preserves_encode_after_update() -> Result<(), Error> {
    // セッションが継続することを確認する: reconfigure 前後で同じセッションを使ってエンコードが連続できる
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;

    let (y, u, v) = build_i420_black_frame();
    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };
    encoder.encode(&frame, &EncodeOptions::default(), 1)?;
    encoder.encode(&frame, &EncodeOptions::default(), 2)?;

    // 動的更新ではセッションを再作成しないため、未出力フレームのフラッシュもしない
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(750_000),
        expected_frame_rate: Some(15),
    })?;

    encoder.encode(&frame, &EncodeOptions::default(), 3)?;
    encoder.encode(&frame, &EncodeOptions::default(), 4)?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 4);
    assert_eq!(callbacks.len(), 4);

    // 受け取った user_data が入力順に並ぶことを確認する (B フレーム未使用のため順序維持される)
    let user_data = callbacks
        .into_iter()
        .map(|r| match r {
            Ok(frame) => frame.user_data,
            Err(e) => panic!("unexpected encode callback error: {e}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(user_data, vec![1, 2, 3, 4]);
    Ok(())
}

#[test]
fn encode_rejects_insufficient_nv12_uv_plane() -> Result<(), Error> {
    let mut config = minimal_encoder_config();
    config.pixel_format = PixelFormat::Nv12;
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(config, {
        let results = Arc::clone(&results);
        move |result| {
            results.lock().expect("results mutex poisoned").push(result);
        }
    })?;
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
