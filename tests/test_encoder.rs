//! `encoder` モジュール (src/encoder.rs と src/encoder/) に対応する単体テスト

mod helpers;

use std::{
    ffi::c_void,
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, channel},
    },
    thread,
    time::{Duration, Instant},
};

use shiguredo_video_toolbox::{
    CodecConfig, DataRateLimit, DecodedFrame, Decoder, DecoderCodec, DecoderConfig, EncodeHandler,
    EncodeOptions, EncodedFrame, Encoder, EncoderConfig, Error, FnDecodeHandler, FnEncodeHandler,
    FrameData, H264EncoderConfig, H264EntropyMode, H264Profile, HevcEncoderConfig, HevcProfile,
    PictureType, PixelFormat, ReconfigureParams, Timestamp,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 480;
const SIZE: usize = WIDTH as usize * HEIGHT as usize;
type EncodeResult<T> = Result<EncodedFrame<T>, Error>;
type SharedEncodeResults<T> = Arc<Mutex<Vec<EncodeResult<T>>>>;

/// `sys` モジュールは private のため、`Encoder::encode_pixel_buffer` に渡す CVPixelBuffer を
/// 用意するために CoreVideo の関数をテスト側で宣言する。宣言は bindgen が生成する
/// `src/sys.rs` のシグネチャと一致させること。
mod cv {
    use std::ffi::c_void;

    /// CFAllocatorRef (null 許容)
    pub type CFAllocatorRef = *const c_void;
    /// CFDictionaryRef (null 許容)
    pub type CFDictionaryRef = *const c_void;
    /// CVImageBufferRef / CVPixelBufferRef
    pub type CVImageBufferRef = *mut c_void;
    /// CVReturn
    pub type CVReturn = i32;

    /// kCVPixelFormatType_32BGRA
    pub const PIXEL_FORMAT_32BGRA: u32 = 0x4247_5241;
    /// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange (FourCC '420v')
    pub const PIXEL_FORMAT_420_BIPLANAR_VIDEO_RANGE: u32 = 0x3432_3076;
    /// kCVPixelFormatType_420YpCbCr8Planar (FourCC 'y420')
    pub const PIXEL_FORMAT_420_PLANAR: u32 = 0x7934_3230;

    unsafe extern "C" {
        /// 指定サイズ・ピクセルフォーマットの CVPixelBuffer を生成する
        pub fn CVPixelBufferCreate(
            allocator: CFAllocatorRef,
            width: usize,
            height: usize,
            pixel_format_type: u32,
            pixel_buffer_attributes: CFDictionaryRef,
            pixel_buffer_out: *mut CVImageBufferRef,
        ) -> CVReturn;

        /// CVPixelBuffer の参照カウントを 1 減らす
        pub fn CVPixelBufferRelease(pixel_buffer: CVImageBufferRef);
    }
}

/// 指定サイズ・ピクセルフォーマットの CVPixelBuffer を生成し、`f` に生ポインタを渡す
///
/// `f` の実行中はバッファを所有し、終了後に `CVPixelBufferRelease` で解放する。
/// `f` の中で `CVPixelBufferRetain` されることを前提とした使い方を想定している。
fn with_pixel_buffer<T>(
    width: u32,
    height: u32,
    pixel_format_type: u32,
    f: impl FnOnce(*mut c_void) -> T,
) -> T {
    let mut image_buffer: cv::CVImageBufferRef = std::ptr::null_mut();
    let status = unsafe {
        cv::CVPixelBufferCreate(
            std::ptr::null(),
            width as usize,
            height as usize,
            pixel_format_type,
            std::ptr::null(),
            &mut image_buffer,
        )
    };
    assert_eq!(status, 0, "CVPixelBufferCreate が失敗した: status={status}");
    assert!(!image_buffer.is_null(), "CVPixelBuffer が NULL になった");

    let result = f(image_buffer.cast::<c_void>());

    unsafe { cv::CVPixelBufferRelease(image_buffer) };
    result
}

fn minimal_encoder_config() -> EncoderConfig {
    EncoderConfig {
        width: 640,
        height: 480,
        codec: CodecConfig::H264(H264EncoderConfig {
            profile: Some(H264Profile::Main),
            entropy_mode: Some(H264EntropyMode::Cabac),
        }),
        pixel_format: PixelFormat::I420,
        average_bitrate: None,
        fps_numerator: 1,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: Some(false),
        real_time: Some(false),
        maximize_power_efficiency: Some(false),
        allow_frame_reordering: Some(false),
        allow_temporal_compression: Some(true),
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
            profile: Some(HevcProfile::Main),
            allow_open_gop: Some(true),
        })
    } else {
        CodecConfig::H264(H264EncoderConfig {
            profile: Some(H264Profile::Main),
            entropy_mode: Some(H264EntropyMode::Cabac),
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
        prioritize_encoding_speed_over_quality: Some(false),
        real_time: Some(false),
        maximize_power_efficiency: Some(false),
        allow_frame_reordering: Some(false),
        allow_temporal_compression: Some(true),
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
        data_rate_limits: Vec::new(),
    }
}

fn build_i420_black_frame() -> ([u8; SIZE], [u8; SIZE / 4], [u8; SIZE / 4]) {
    ([0; SIZE], [0; SIZE / 4], [0; SIZE / 4])
}

/// `minimal_encoder_config` (640x480) に合わせた黒の I420 フレームを生成する
fn build_i420_black_frame_640x480() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w: usize = 640;
    let h: usize = 480;
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);
    (
        vec![0u8; w * h],
        vec![0u8; uv_w * uv_h],
        vec![0u8; uv_w * uv_h],
    )
}

/// エンコード結果を無視するハンドラー (構築・設定の検証だけで完結するテスト用)
fn noop_encode_handler() -> FnEncodeHandler<()> {
    FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
}

/// 検証エラーを期待して `reconfigure` を呼び、返された `Error` を取り出す
///
/// `Encoder::reconfigure` は拒否時に `self.config` を変更しない契約のため、呼び出しの
/// 前後で `ReconfigureParams` が触れ得るフィールド (`average_bitrate` / `fps_numerator` /
/// `fps_denominator` / `data_rate_limits`) が初期値のまま保たれることもここで検証する。
/// 拒否系テストはすべてこのヘルパーを経由するため、エラー種別の検証に加えて設定の
/// 不変性が全ケースで確認される。
fn reconfigure_err(params: ReconfigureParams) -> Error {
    let mut encoder = Encoder::new(encoder_config(false), noop_encode_handler())
        .expect("エンコーダーの構築が成功すること");
    // 検証エラーは `self` に触れる前に返るため、呼び出し前の値をそのまま期待値にできる。
    let before_bitrate = encoder.config().average_bitrate;
    let before_fps_numerator = encoder.config().fps_numerator;
    let before_fps_denominator = encoder.config().fps_denominator;
    let before_data_rate_limits = encoder.config().data_rate_limits.clone();

    let error = encoder
        .reconfigure(params)
        .expect_err("無効なパラメータが拒否されること");

    assert_eq!(
        encoder.config().average_bitrate,
        before_bitrate,
        "拒否された reconfigure が average_bitrate を変更している"
    );
    assert_eq!(
        encoder.config().fps_numerator,
        before_fps_numerator,
        "拒否された reconfigure が fps_numerator を変更している"
    );
    assert_eq!(
        encoder.config().fps_denominator,
        before_fps_denominator,
        "拒否された reconfigure が fps_denominator を変更している"
    );
    assert_eq!(
        encoder.config().data_rate_limits,
        before_data_rate_limits,
        "拒否された reconfigure が data_rate_limits を変更している"
    );

    error
}

fn wait_and_take_results<T>(
    results: &SharedEncodeResults<T>,
    min_count: usize,
) -> Vec<EncodeResult<T>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .len()
            >= min_count
        {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    let mut guard = results
        .lock()
        .expect("結果バッファの mutex が poison になっている");
    std::mem::take(&mut *guard)
}

/// NV12 のデコード結果から取り出したプレーン
///
/// `Nv12Frame` のプレーンはデコードコールバックを抜けると無効になるため、コピーして保持する。
/// エンコード結果をデコードし直し、入力したプレーンの内容が変化していないかを確認するために使う。
enum DecodedNv12 {
    /// NV12 でデコードされた結果
    Frame {
        /// Y プレーン
        y_plane: Vec<u8>,
        /// Y プレーンのストライド
        y_stride: usize,
        /// UV プレーン
        uv_plane: Vec<u8>,
        /// UV プレーンのストライド
        uv_stride: usize,
    },
    /// NV12 以外のフォーマットでデコードされた (テストの前提が壊れている)
    UnexpectedFormat,
    /// デコードに失敗した
    Failed(String),
}

/// NV12 のデコード結果をテスト本体のスレッドへ受け流すチャネルを返す
///
/// `DecodeHandler` は Video Toolbox のコールバックスレッドから呼ばれるため、結果を
/// `mpsc::channel` でテスト本体のスレッドへ渡す。`DecodedFrame` は `Send` ではなく、
/// コールバックを抜けるとプレーンも無効になるため、プレーンをコピーして送る。
fn nv12_decode_collector() -> (Receiver<DecodedNv12>, FnDecodeHandler<u64>) {
    let (sender, receiver) = channel();
    let handler = FnDecodeHandler::new(move |result: Result<DecodedFrame<u64>, Error>| {
        let decoded = match result {
            Ok(DecodedFrame::Nv12 { frame, .. }) => DecodedNv12::Frame {
                y_plane: frame.y_plane().to_vec(),
                y_stride: frame.y_stride(),
                uv_plane: frame.uv_plane().to_vec(),
                uv_stride: frame.uv_stride(),
            },
            Ok(DecodedFrame::I420 { .. }) => DecodedNv12::UnexpectedFormat,
            Err(e) => DecodedNv12::Failed(format!("デコードに失敗した: {e}")),
        };
        if sender.send(decoded).is_err() {
            eprintln!("デコード結果の受信側が既に破棄されている");
        }
    });
    (receiver, handler)
}

/// デコードコールバックの結果を 1 件受け取る
fn recv_decoded_nv12(receiver: &Receiver<DecodedNv12>) -> DecodedNv12 {
    receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("デコードコールバックが 3 秒以内に届くこと")
}

/// 16 と 235 の 2 値だけで構成したプレーンが、欠落やずれなく転送されていることを検証する
///
/// 非可逆圧縮で値そのものは変化するが、この 2 値が入れ替わることはないため、
/// 中間値の 128 を境にした大小関係で判定する。転送されなかった領域は 0 のまま残るので、
/// 期待値が 235 の位置が 0 になれば検出できる。
///
/// `original_stride` / `decoded_stride` はそれぞれの行の先頭から次の行の先頭までのバイト数で、
/// 有効な `width` バイトだけを比較する。
fn assert_binary_plane_matches(
    original: &[u8],
    original_stride: usize,
    decoded: &[u8],
    decoded_stride: usize,
    width: usize,
    height: usize,
    label: &str,
) {
    assert!(
        decoded_stride >= width,
        "{label} プレーンのストライドが幅より小さい: {decoded_stride} < {width}"
    );
    assert!(
        decoded.len() >= height * decoded_stride,
        "{label} プレーンの長さが不足している: {} < {}",
        decoded.len(),
        height * decoded_stride
    );
    for row in 0..height {
        for col in 0..width {
            let expected = original[row * original_stride + col];
            let actual = decoded[row * decoded_stride + col];
            let matched = if expected >= 128 {
                actual >= 128
            } else {
                actual < 128
            };
            assert!(
                matched,
                "{label} プレーンの内容が入力と一致しない: 行 {row} 列 {col} 期待 {expected} 実際 {actual}"
            );
        }
    }
}

fn encode_black_frame_roundtrip(is_h265: bool) -> Result<(), Error> {
    let config = encoder_config(is_h265);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
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
        7,
    )?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match callbacks
        .into_iter()
        .next()
        .expect("コールバック結果が届いていない")
    {
        Ok(frame) => {
            assert_eq!(frame.user_data, 7);
            assert!(!frame.data.is_empty());
        }
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    }

    Ok(())
}

/// 黒フレーム 1 枚を H.264 でエンコードし、出力コールバックが 1 回呼ばれて
/// user_data とエンコード済みデータが届くことを検証する。ビットストリームの内容は
/// 検証しないため、決定的で圧縮が効く黒フレームを使う
#[test]
fn encode_h264_black() -> Result<(), Error> {
    encode_black_frame_roundtrip(false)
}

/// 黒フレーム 1 枚を H.265 でエンコードし、出力コールバックが 1 回呼ばれて
/// user_data とエンコード済みデータが届くことを検証する。ビットストリームの内容は
/// 検証しないため、決定的で圧縮が効く黒フレームを使う
#[test]
fn encode_h265_black() -> Result<(), Error> {
    encode_black_frame_roundtrip(true)
}

/// 2 フレームを連続でエンコードし、各フレームに渡した user_data (10 / 20) が
/// そのままコールバックに届くことを検証する。allow_frame_reordering: Some(false) のため
/// 投入順でコールバックされるが (Video Toolbox の保証ではない)、防衛的に取得後に
/// ソートして比較する
#[test]
fn callback_keeps_user_data_per_frame() -> Result<(), Error> {
    let config = encoder_config(false);
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
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
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        })
        .collect::<Vec<_>>();
    user_data.sort_unstable();
    assert_eq!(user_data, vec![10, 20]);

    Ok(())
}

/// フレーム再順序付けを有効にしたときに現れると想定する B フレームの下限数
///
/// Video Toolbox は常に B フレームを生成するわけではないため、実測値より小さな値を下限にする。
/// この値なら B フレームの判定が壊れたときに 2 種類の設定 (H.264 / H.265) の両方で検出できる
/// (12 フレームの実測は H.264 が 4 枚、H.265 が 5 枚)。
const MIN_B_FRAMES_WITH_REORDERING: usize = 2;

/// 合成フレーム 12 枚をエンコードし、各出力の時刻とピクチャータイプを検証する
///
/// - 有効な時刻がすべての出力で取得でき、時刻を秒に直して昇順に並べると狭義単調増加する
///   (提示時刻そのものはフレーム再順序付けを有効にすると出力順に並ばないため並べ替えて検証する)
/// - 最初の出力のピクチャータイプが `I` である
/// - フレーム再順序付けが無効 (`Some(false)`) な場合は `B` が現れず、有効 (`Some(true)`) な
///   場合と未指定 (`None`) の場合は [`MIN_B_FRAMES_WITH_REORDERING`] 枚以上の `B` が現れる
///
/// `None` は `EncoderConfig::allow_frame_reordering` を設定しないことを意味するため、
/// Video Toolbox の既定 (フレーム再順序付け有効) で `B` フレームが生成されることも
/// ここで検証する。
fn assert_picture_type_and_timestamp(is_h265: bool, reorder: Option<bool>) -> Result<(), Error> {
    const FRAMES: u64 = 12;
    const FPS: u32 = 30;

    let mut config = encoder_config(is_h265);
    config.average_bitrate = Some(2_000_000);
    config.allow_frame_reordering = reorder;
    config.fps_numerator = FPS;
    config.fps_denominator = 1;
    config.max_key_frame_interval = Some(NonZeroU32::new(10).expect("10 は非ゼロ"));

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let mut seed = 0x1234_5678_9abc_def0u64;
    for i in 0..FRAMES {
        let (y, u, v) = synthetic_i420_frame(i as usize, &mut seed);
        encoder.encode(
            &FrameData::I420 {
                y: &y,
                u: &u,
                v: &v,
            },
            &EncodeOptions::default(),
            i,
        )?;
    }
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, FRAMES as usize);
    assert_eq!(
        callbacks.len(),
        FRAMES as usize,
        "投入したフレームすべてのエンコード結果が届くこと"
    );
    let frames = callbacks
        .into_iter()
        .map(|callback| match callback {
            Ok(frame) => frame,
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        })
        .collect::<Vec<_>>();

    // 出力順は投入順の frame_idx で引ける (Video Toolbox の保証ではなく実測前提)。
    // user_data に投入時のフレーム番号を入れているため、最初の出力は frame_idx = 0 である。
    assert_eq!(
        frames[0].user_data, 0,
        "最初の出力は最初に投入したフレームであること"
    );
    assert_eq!(
        frames[0].picture_type,
        PictureType::I,
        "最初の出力のピクチャータイプは I であること"
    );

    // 各出力の時刻の目盛りは、エンコーダーが入力フレームのタイムスタンプに使った値と一致する
    let times = frames
        .iter()
        .map(|frame| {
            let timestamp = frame.timestamp.expect("有効な提示時刻が取得できること") as Timestamp;
            assert_eq!(
                timestamp.timescale, FPS as i32,
                "時刻の目盛りは EncoderConfig::fps_numerator と一致すること"
            );
            timestamp.seconds()
        })
        .collect::<Vec<_>>();

    // 提示時刻はフレーム再順序付けを有効にすると出力順に並ばないため、並べ替えて単調性を見る
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("時刻に NaN が含まれないこと"));
    for pair in sorted.windows(2) {
        assert!(
            pair[1] > pair[0],
            "提示時刻が狭義単調増加すること: {:?} の並びで {} <= {} になった",
            sorted,
            pair[0],
            pair[1]
        );
    }

    // ピクチャータイプは I (キーフレーム) と B 以外が P になる。キーフレームは
    // max_key_frame_interval の指定と最初のフレームによって複数あり得るため、
    // I の枚数は出力から数える。
    let keyframes = frames
        .iter()
        .filter(|frame| frame.picture_type == PictureType::I)
        .count();
    assert!(
        keyframes >= 1,
        "キーフレームが 1 枚以上あること (実際は {keyframes} 枚)"
    );

    let b_frames = frames
        .iter()
        .filter(|frame| frame.picture_type == PictureType::B)
        .count();
    match reorder {
        Some(true) => assert!(
            b_frames >= MIN_B_FRAMES_WITH_REORDERING,
            "フレーム再順序付けを有効にすると B フレームが {MIN_B_FRAMES_WITH_REORDERING} 枚以上現れること (実際は {b_frames} 枚)"
        ),
        Some(false) => assert_eq!(
            b_frames, 0,
            "フレーム再順序付けを無効にすると B フレームは現れないこと"
        ),
        None => assert!(
            b_frames >= MIN_B_FRAMES_WITH_REORDERING,
            "フレーム再順序付けを未指定にすると Video Toolbox の既定で B フレームが {MIN_B_FRAMES_WITH_REORDERING} 枚以上現れること (実際は {b_frames} 枚)"
        ),
    }
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.picture_type == PictureType::P)
            .count(),
        FRAMES as usize - keyframes - b_frames,
        "I でも B でもない出力は P であること"
    );

    Ok(())
}

/// H.264 でフレーム再順序付けを無効にしたときに、時刻の目盛りが設定と一致し
/// ピクチャータイプが I / P だけになることを検証する
#[test]
fn encode_h264_reports_picture_type_without_reordering() -> Result<(), Error> {
    assert_picture_type_and_timestamp(false, Some(false))
}

/// H.264 でフレーム再順序付けを有効にしたときに、提示時刻が出力順に並ばず
/// B フレームが現れることを検証する
#[test]
fn encode_h264_reports_picture_type_with_reordering() -> Result<(), Error> {
    assert_picture_type_and_timestamp(false, Some(true))
}

/// H.264 でフレーム再順序付けを未指定にしたときに、Video Toolbox の既定で
/// B フレームが現れることを検証する
///
/// `EncoderConfig::allow_frame_reordering` に `None` を指定した場合は
/// `kVTCompressionPropertyKey_AllowFrameReordering` を設定しない。Video Toolbox の既定は
/// フレーム再順序付け有効であり、B フレームが生成される。
/// このテストが失敗するようになった場合は、未指定のプロパティに Video Toolbox の既定が
/// 使われなくなった (crate が独自の既定を押し付けている) ことを意味する。
#[test]
fn encode_h264_reports_b_frames_when_frame_reordering_unspecified() -> Result<(), Error> {
    assert_picture_type_and_timestamp(false, None)
}

/// H.265 でフレーム再順序付けを無効にしたときに、時刻の目盛りが設定と一致し
/// ピクチャータイプが I / P だけになることを検証する
#[test]
fn encode_h265_reports_picture_type_without_reordering() -> Result<(), Error> {
    assert_picture_type_and_timestamp(true, Some(false))
}

/// H.265 でフレーム再順序付けを有効にしたときに、提示時刻が出力順に並ばず
/// B フレームが現れることを検証する
#[test]
fn encode_h265_reports_picture_type_with_reordering() -> Result<(), Error> {
    assert_picture_type_and_timestamp(true, Some(true))
}

/// エンコード結果の提示時刻が、`Encoder::reconfigure` でフレームレートを変更した後も
/// 物理時間として単調増加し、変更後の時刻の目盛りが新しいフレームレートになることを検証する
///
/// 30000/1001 (約 29.97 fps) で 2 フレーム投入してから 60 fps へ変更し、さらに 2 フレーム
/// 投入する。提示時刻は投入したフレームのものなので、変更をまたいでも投入順に並ぶ
/// (`allow_frame_reordering: Some(false)`)。
#[test]
fn encode_timestamp_is_monotonic_across_reconfigure() -> Result<(), Error> {
    const FPS_NUMERATOR: u32 = 30_000;
    const FPS_DENOMINATOR: u32 = 1_001;
    const NEW_FPS: u32 = 60;

    let mut config = encoder_config(false);
    config.average_bitrate = Some(2_000_000);
    config.fps_numerator = FPS_NUMERATOR;
    config.fps_denominator = FPS_DENOMINATOR;

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };
    for user_data in 0..2u64 {
        encoder.encode(&frame, &EncodeOptions::default(), user_data)?;
    }
    encoder.reconfigure(ReconfigureParams {
        expected_frame_rate: Some(NEW_FPS),
        ..Default::default()
    })?;
    for user_data in 2..4u64 {
        encoder.encode(&frame, &EncodeOptions::default(), user_data)?;
    }
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 4);
    assert_eq!(callbacks.len(), 4, "投入した 4 フレームの出力が届くこと");

    let mut timestamps = Vec::new();
    for callback in callbacks {
        let frame = match callback {
            Ok(frame) => frame,
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        };
        timestamps.push(frame.timestamp.expect("有効な提示時刻が取得できること") as Timestamp);
    }

    // 変更前の 2 フレームは元の目盛り、変更後の 2 フレームは新しい目盛りで通知される
    assert_eq!(
        timestamps[0].timescale, FPS_NUMERATOR as i32,
        "変更前のフレームの目盛りは元のフレームレートの分子であること"
    );
    assert_eq!(
        timestamps[1].timescale, FPS_NUMERATOR as i32,
        "変更前のフレームの目盛りは元のフレームレートの分子であること"
    );
    assert_eq!(
        timestamps[2].timescale, NEW_FPS as i32,
        "変更後のフレームの目盛りは新しいフレームレートであること"
    );
    assert_eq!(
        timestamps[3].timescale, NEW_FPS as i32,
        "変更後のフレームの目盛りは新しいフレームレートであること"
    );

    // 目盛りが変わるため、それぞれを秒に直して比較する
    let seconds = timestamps
        .iter()
        .map(|timestamp| timestamp.seconds())
        .collect::<Vec<_>>();
    for pair in seconds.windows(2) {
        assert!(
            pair[1] > pair[0],
            "フレームレートの変更をまたいでも提示時刻が狭義単調増加すること: {seconds:?}"
        );
    }
    assert!(
        seconds[0] < 1.0 / FPS_NUMERATOR as f64 * FPS_DENOMINATOR as f64,
        "最初のフレームの提示時刻は 1 フレームぶん未満であること: {seconds:?}"
    );

    Ok(())
}

/// width に 0 を指定した Encoder::new が InvalidConfig (field: width) で拒否されることを検証する
/// (Video Toolbox の寸法は正の値が必要なため)
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

/// height に 0 を指定した Encoder::new が InvalidConfig (field: height) で拒否されることを検証する
/// (Video Toolbox の寸法は正の値が必要なため)
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

/// fps_numerator が i32::MAX を超えると InvalidConfig で拒否されることを検証する
/// (CMTime の timescale は i32 のため)
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

/// width が i32::MAX を超えると InvalidConfig で拒否されることを検証する
/// (Video Toolbox の寸法引数は i32 のため)
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

/// height が i32::MAX を超えると InvalidConfig で拒否されることを検証する
/// (Video Toolbox の寸法引数は i32 のため)
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

/// average_bitrate が i64::MAX を超えると InvalidConfig で拒否されることを検証する
/// (CFNumber は SInt64 のため)
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

/// fps_denominator に 0 を指定すると InvalidConfig で拒否されることを検証する (0 除算を防ぐため)
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

/// fps_numerator に 0 を指定すると InvalidConfig で拒否されることを検証する (フレームレート 0 は無効なため)
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

/// fps_numerator と average_bitrate を同時に不正にした Encoder::new が
/// fps_numerator のエラーを先に返すことを検証する
///
/// 構築 (Encoder::new) と再設定 (Encoder::reconfigure) で検証順序が揃っていることを
/// 固定するテスト。既存のフィールド単位の拒否テストは各エラー種別しか見ていないため、
/// 共通の検証関数の中で順序が逆転しても検出できない。
#[test]
fn encoder_rejects_prioritizing_fps_error_over_bitrate_error() {
    let mut c = minimal_encoder_config();
    // 両フィールドを不正値にして、どちらのエラーが優先されるかを判定する
    c.fps_numerator = 0;
    c.average_bitrate = Some(0);
    assert!(matches!(
        Encoder::new(c, noop_encode_handler()),
        Err(Error::InvalidConfig { field, reason })
            if field == "fps_numerator" && reason == "must not be zero"
    ));
}

/// I420 の Y プレーン長が不足していると InsufficientFrameData で拒否され、
/// コールバックが発火しないことを検証する
#[test]
fn encode_rejects_insufficient_i420_y_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// I420 の U プレーン長が不足していると InsufficientFrameData で拒否され、
/// コールバックが発火しないことを検証する
#[test]
fn encode_rejects_insufficient_i420_u_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// I420 設定のエンコーダーに Nv12 フレームを渡すと PixelFormatMismatch で拒否され、
/// コールバックが発火しないことを検証する
#[test]
fn encode_rejects_pixel_format_mismatch_i420_encoder_with_nv12_frame() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// Nv12 設定のエンコーダーに Nv12 フレームを渡すと、2 プレーンの CVPixelBuffer を生成して
/// エンコードが成功することを検証する (I420 経路だけが通っていた Nv12 のプラナーコピーを検証する)
#[test]
fn encode_nv12_frame_succeeds() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_nv12_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    // 640x480 の NV12: Y が 640*480、UV が 640*240
    let y = vec![0u8; 640 * 480];
    let uv = vec![128u8; 640 * 240];
    enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
        42,
    )?;
    enc.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match &callbacks[0] {
        Ok(frame) => {
            assert_eq!(frame.user_data, 42);
            assert!(
                !frame.data.is_empty(),
                "Nv12 フレームのエンコード結果が空になっている"
            );
        }
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    }
    Ok(())
}

/// Nv12 設定のエンコーダーに Nv12 フレームを渡したとき、Y / UV プレーンの内容が
/// 欠落せずにエンコードされていることを、デコード結果との比較で検証する
///
/// `Encoder::encode` は入力プレーンを `CVPixelBuffer` へコピーするが、コピー幅を誤っても
/// エンコード自体は成功し、結果のビットストリームも空にならない。そのため
/// `encode_nv12_frame_succeeds` ではこの不具合を検出できない。エンコード結果をデコードし直して
/// 入力プレーンと比較することで、コピーの欠落を検出する。
///
/// プレーンは 16 と 235 の 2 値だけで構成する。UV プレーンの各行の後半が転送されない不具合では
/// 右半分が 0 のまま残るため、2 値の大小関係が崩れて検出できる。
#[test]
fn encode_nv12_frame_preserves_plane_content() -> Result<(), Error> {
    const W: usize = 640;
    const H: usize = 480;
    // 4:2:0 なので UV プレーンの行数は Y の半分になる
    const CHROMA_H: usize = H / 2;

    // Y は左半分を 16、右半分を 235 にする。Y プレーンのコピーは元から正しいため、
    // この検証が通ることはエンコードとデコードの経路自体が生きていることの対照になる。
    let mut y = vec![16u8; W * H];
    for row in 0..H {
        for col in W / 2..W {
            y[row * W + col] = 235;
        }
    }

    // UV は偶数バイト目が U、奇数バイト目が V のインターリーブ。
    // 左半分を U=16 / V=235、右半分を U=235 / V=16 にする。U と V を左右で入れ替えているため、
    // U / V の取り違えも検出できる。
    let mut uv = vec![16u8; W * CHROMA_H];
    for row in 0..CHROMA_H {
        for col in 0..W {
            let is_u = col % 2 == 0;
            let is_left = col < W / 2;
            let high = if is_u { !is_left } else { is_left };
            uv[row * W + col] = if high { 235 } else { 16 };
        }
    }

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        minimal_nv12_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;
    // パラメータセットをデコーダーに渡すため、キーフレームとしてエンコードする
    encoder.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions {
            force_key_frame: true,
        },
        0,
    )?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1, "エンコードコールバックは 1 件届くこと");
    let encoded = match &callbacks[0] {
        Ok(frame) => frame,
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    };
    assert_eq!(encoded.sps_list.len(), 1, "SPS は 1 組得られること");
    assert_eq!(encoded.pps_list.len(), 1, "PPS は 1 組得られること");

    let (receiver, handler) = nv12_decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: &encoded.sps_list[0],
                pps: &encoded.pps_list[0],
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::Nv12,
        },
        handler,
    )?;
    decoder.decode(&encoded.data, 0)?;
    decoder.finish()?;

    let event = recv_decoded_nv12(&receiver);
    let (y_plane, y_stride, uv_plane, uv_stride) = match &event {
        DecodedNv12::Frame {
            y_plane,
            y_stride,
            uv_plane,
            uv_stride,
        } => (y_plane, *y_stride, uv_plane, *uv_stride),
        DecodedNv12::UnexpectedFormat => {
            panic!("NV12 のデコード結果を期待したが I420 が届いた")
        }
        DecodedNv12::Failed(message) => panic!("デコード結果が得られていない: {message}"),
    };

    assert_binary_plane_matches(&y, W, y_plane, y_stride, W, H, "Y");
    assert_binary_plane_matches(&uv, W, uv_plane, uv_stride, W, CHROMA_H, "UV");
    Ok(())
}

/// `Encoder::encode_pixel_buffer` に I420 の CVPixelBuffer を渡すとエンコードが成功し、
/// `user_data` がそのままコールバックに届くことを検証する
#[test]
fn encode_pixel_buffer_submits_valid_i420_buffer() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    with_pixel_buffer(640, 480, cv::PIXEL_FORMAT_420_PLANAR, |image_buffer| {
        // SAFETY: `image_buffer` は有効な CVPixelBuffer であり、`encode_pixel_buffer` は
        // 内部で CFRetain するため、このスコープを抜けて解放されても問題ない
        unsafe { encoder.encode_pixel_buffer(image_buffer, &EncodeOptions::default(), 7) }
    })?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match &callbacks[0] {
        Ok(frame) => {
            assert_eq!(frame.user_data, 7);
            assert!(
                !frame.data.is_empty(),
                "encode_pixel_buffer のエンコード結果が空になっている"
            );
        }
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    }
    Ok(())
}

/// `Encoder::encode_pixel_buffer` に I420 / Nv12 のいずれでもない FourCC の CVPixelBuffer を
/// 渡すと UnknownPixelFormat で拒否され、実際の FourCC が診断情報として返ることを検証する
#[test]
fn encode_pixel_buffer_rejects_unknown_pixel_format() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let result = with_pixel_buffer(640, 480, cv::PIXEL_FORMAT_32BGRA, |image_buffer| {
        // SAFETY: `image_buffer` は有効な CVPixelBuffer
        unsafe { encoder.encode_pixel_buffer(image_buffer, &EncodeOptions::default(), 1) }
    });
    let err = result.expect_err("32BGRA の CVPixelBuffer は未知のフォーマットとして拒否されること");
    assert!(matches!(
        err,
        Error::UnknownPixelFormat {
            expected: PixelFormat::I420,
            fourcc,
        } if fourcc == cv::PIXEL_FORMAT_32BGRA
    ));
    // 画素フォーマットの検証で拒否されるためフレームは送信されない
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// I420 設定のエンコーダーに Nv12 の CVPixelBuffer を渡すと
/// PixelFormatMismatch (expected: I420 / actual: Nv12) で拒否されることを検証する
#[test]
fn encode_pixel_buffer_rejects_pixel_format_mismatch() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        minimal_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let result = with_pixel_buffer(
        640,
        480,
        cv::PIXEL_FORMAT_420_BIPLANAR_VIDEO_RANGE,
        |image_buffer| {
            // SAFETY: `image_buffer` は有効な CVPixelBuffer
            unsafe { encoder.encode_pixel_buffer(image_buffer, &EncodeOptions::default(), 1) }
        },
    );
    let err = result.expect_err("Nv12 の CVPixelBuffer はフォーマット不一致として拒否されること");
    assert!(matches!(
        err,
        Error::PixelFormatMismatch {
            expected: PixelFormat::I420,
            actual: PixelFormat::Nv12,
        }
    ));
    // 画素フォーマットの検証で拒否されるためフレームは送信されない
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// H.264 のキーフレーム出力がピクチャータイプ `I` かつ SPS / PPS 1 組ずつを持ち、
/// 2 枚目の非キーフレームにはパラメータセットが付かないことを検証する
#[test]
fn h264_keyframe_carries_parameter_sets() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };
    // 1 枚目は明示的にキーフレームを要求し、2 枚目は既定 (キーフレーム指定なし) で送る
    encoder.encode(
        &frame,
        &EncodeOptions {
            force_key_frame: true,
        },
        1,
    )?;
    encoder.encode(&frame, &EncodeOptions::default(), 2)?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 2);
    assert_eq!(callbacks.len(), 2);
    let frames: Vec<EncodedFrame<u64>> = callbacks
        .into_iter()
        .map(|r| match r {
            Ok(frame) => frame,
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        })
        .collect();

    let keyframe = frames
        .iter()
        .find(|f| f.user_data == 1)
        .expect("1 枚目のエンコード結果が届いていない");
    assert!(
        keyframe.picture_type == PictureType::I,
        "force_key_frame の結果がキーフレームになっていない"
    );
    assert_eq!(keyframe.sps_list.len(), 1, "キーフレームには SPS が付く");
    assert_eq!(keyframe.pps_list.len(), 1, "キーフレームには PPS が付く");
    assert!(keyframe.vps_list.is_empty(), "H.264 に VPS は無い");
    assert!(!keyframe.sps_list[0].is_empty(), "SPS が空になっている");
    assert!(!keyframe.pps_list[0].is_empty(), "PPS が空になっている");

    let delta = frames
        .iter()
        .find(|f| f.user_data == 2)
        .expect("2 枚目のエンコード結果が届いていない");
    assert!(
        delta.picture_type != PictureType::I,
        "2 枚目はキーフレームでないこと"
    );
    assert!(delta.sps_list.is_empty(), "非キーフレームに SPS は付かない");
    assert!(delta.pps_list.is_empty(), "非キーフレームに PPS は付かない");
    assert!(delta.vps_list.is_empty(), "非キーフレームに VPS は付かない");
    Ok(())
}

/// H.265 のキーフレーム出力が VPS / SPS / PPS を 1 組ずつ持ち、
/// 2 枚目の非キーフレームにはパラメータセットが付かないことを検証する
#[test]
fn h265_keyframe_carries_parameter_sets() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        encoder_config(true),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };
    encoder.encode(
        &frame,
        &EncodeOptions {
            force_key_frame: true,
        },
        1,
    )?;
    encoder.encode(&frame, &EncodeOptions::default(), 2)?;
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 2);
    assert_eq!(callbacks.len(), 2);
    let frames: Vec<EncodedFrame<u64>> = callbacks
        .into_iter()
        .map(|r| match r {
            Ok(frame) => frame,
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        })
        .collect();

    let keyframe = frames
        .iter()
        .find(|f| f.user_data == 1)
        .expect("1 枚目のエンコード結果が届いていない");
    assert!(
        keyframe.picture_type == PictureType::I,
        "force_key_frame の結果がキーフレームになっていない"
    );
    assert_eq!(keyframe.vps_list.len(), 1, "キーフレームには VPS が付く");
    assert_eq!(keyframe.sps_list.len(), 1, "キーフレームには SPS が付く");
    assert_eq!(keyframe.pps_list.len(), 1, "キーフレームには PPS が付く");
    assert!(!keyframe.vps_list[0].is_empty(), "VPS が空になっている");
    assert!(!keyframe.sps_list[0].is_empty(), "SPS が空になっている");
    assert!(!keyframe.pps_list[0].is_empty(), "PPS が空になっている");

    let delta = frames
        .iter()
        .find(|f| f.user_data == 2)
        .expect("2 枚目のエンコード結果が届いていない");
    assert!(
        delta.picture_type != PictureType::I,
        "2 枚目はキーフレームでないこと"
    );
    assert!(delta.vps_list.is_empty(), "非キーフレームに VPS は付かない");
    assert!(delta.sps_list.is_empty(), "非キーフレームに SPS は付かない");
    assert!(delta.pps_list.is_empty(), "非キーフレームに PPS は付かない");
    Ok(())
}

/// H.264 の全プロファイル / エントロピー符号化モードの組合せでセッションを構築できることを検証する
#[test]
fn encoder_accepts_all_h264_profiles_and_entropy_modes() {
    for profile in [H264Profile::Baseline, H264Profile::Main, H264Profile::High] {
        for entropy_mode in [H264EntropyMode::Cavlc, H264EntropyMode::Cabac] {
            let mut config = minimal_encoder_config();
            config.codec = CodecConfig::H264(H264EncoderConfig {
                profile: Some(profile),
                entropy_mode: Some(entropy_mode),
            });
            let encoder = Encoder::new(config, noop_encode_handler())
                .unwrap_or_else(|e| panic!("{profile:?} / {entropy_mode:?} の構築に失敗した: {e}"));
            assert_eq!(encoder.config().width, 640);
        }
    }
}
/// H.265 の全プロファイルでセッションを構築できることを検証する
#[test]
fn encoder_accepts_all_hevc_profiles() {
    for profile in [HevcProfile::Main, HevcProfile::Main10] {
        let mut config = minimal_encoder_config();
        config.codec = CodecConfig::Hevc(HevcEncoderConfig {
            profile: Some(profile),
            allow_open_gop: Some(false),
        });
        let encoder = Encoder::new(config, noop_encode_handler())
            .unwrap_or_else(|e| panic!("{profile:?} のセッション構築に失敗した: {e}"));
        assert_eq!(encoder.config().width, 640);
    }
}

/// I420 の V プレーン長が不足していると InsufficientFrameData (plane: V) で拒否されることを検証する
///
/// Y / U の不足は既存テストで検証済みだが、V の検証が欠落していても Y / U のテストでは検出できない。
#[test]
fn encode_rejects_insufficient_i420_v_plane() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_encoder_config(), noop_encode_handler())?;
    let y = vec![0u8; 640 * 480];
    let u = vec![0u8; 320 * 240];
    let v = [0u8; 1];
    let r = enc.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        (),
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane, expected, actual })
            if plane == "V" && expected == 320 * 240 && actual == 1
    ));
    Ok(())
}

/// Nv12 の Y プレーン長が不足していると InsufficientFrameData (plane: Y) で拒否されることを検証する
///
/// UV プレーンの不足は既存テストで検証済みだが、Y の検証が欠落していても UV のテストでは検出できない。
#[test]
fn encode_rejects_insufficient_nv12_y_plane() -> Result<(), Error> {
    let mut enc = Encoder::new(minimal_nv12_encoder_config(), noop_encode_handler())?;
    let y = [0u8; 1];
    let uv = vec![0u8; 640 * 240];
    let r = enc.encode(
        &FrameData::Nv12 { y: &y, uv: &uv },
        &EncodeOptions::default(),
        (),
    );
    assert!(matches!(
        r,
        Err(Error::InsufficientFrameData { plane, expected, actual })
            if plane == "Y" && expected == 640 * 480 && actual == 1
    ));
    Ok(())
}

/// `allow_open_gop: false` / `allow_temporal_compression: false` のセッションでも
/// エンコードが成功することを検証する
///
/// どちらも false のときにのみ Video Toolbox のプロパティを設定する分岐を持つため、
/// true の組み合わせだけでは分岐が実行されない。
#[test]
fn encoder_accepts_disabled_open_gop_and_temporal_compression() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut config = encoder_config(true);
    config.allow_temporal_compression = Some(false);
    config.max_key_frame_interval = std::num::NonZeroU32::new(30);
    config.codec = CodecConfig::Hevc(HevcEncoderConfig {
        profile: Some(HevcProfile::Main),
        allow_open_gop: Some(false),
    });
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
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
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 1);
    assert_eq!(callbacks.len(), 1);
    match &callbacks[0] {
        Ok(frame) => assert!(!frame.data.is_empty()),
        Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
    }
    Ok(())
}

/// `data_rate_limits` に 2 個のリミットを指定したセッションを構築できることを検証する
///
/// Video Toolbox の仕様上指定できるのは 0〜2 個で、3 個以上の拒否は既存テストで検証済みだが、
/// 上限個数 (2 個) の受理は検証されていない。
#[test]
fn encoder_accepts_two_data_rate_limits() -> Result<(), Error> {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![
        DataRateLimit {
            bytes: 93_750,
            window: Duration::from_secs(1),
        },
        DataRateLimit {
            bytes: 187_500,
            window: Duration::from_secs(2),
        },
    ];
    let encoder = Encoder::new(config, noop_encode_handler())?;
    assert_eq!(encoder.config().data_rate_limits.len(), 2);
    Ok(())
}

/// `Encoder::config()` が H.265 / Nv12 の設定も入力どおりに返すことを検証する
///
/// 既存の `encoder_config_returns_initial_value` は H.264 / I420 のみを検証しているため、
/// `CodecConfig::Hevc` バリアントと `PixelFormat::Nv12` の保持も固定する。
#[test]
fn encoder_config_returns_hevc_and_nv12_values() -> Result<(), Error> {
    let mut config = minimal_nv12_encoder_config();
    config.codec = CodecConfig::Hevc(HevcEncoderConfig {
        profile: Some(HevcProfile::Main10),
        allow_open_gop: Some(true),
    });
    let encoder = Encoder::new(config, noop_encode_handler())?;

    assert_eq!(encoder.config().pixel_format, PixelFormat::Nv12);
    match &encoder.config().codec {
        CodecConfig::Hevc(hevc) => {
            assert_eq!(hevc.profile, Some(HevcProfile::Main10));
            assert_eq!(hevc.allow_open_gop, Some(true));
        }
        other => panic!("Hevc バリアントが保持されていない: {other:?}"),
    }
    Ok(())
}

/// ユーザー定義の [`EncodeHandler`] 実装が、独自の `UserData` / `Error` 型で動作することを検証する
///
/// 本クレートはバックエンド間でコールバックモデルを揃えるため `EncodeHandler` トレイトを
/// 公開しているが、既存テストは `FnEncodeHandler` しか使っていない。トレイトの契約
/// (`type UserData` / `type Error: From<Error>` / `on_encoded`) を独自実装で固定する。
/// なおエンコードコールバックが `Err` になる経路 (フレームドロップ等) は公開 API から
/// 確実に再現できないため、ここでは正常系のみを検証する。
#[test]
fn custom_encode_handler_receives_user_data() -> Result<(), Error> {
    /// テスト内で使う結果バッファの型 (型の入れ子が深いため別名を付ける)
    type Results = Arc<Mutex<Vec<Result<EncodedFrame<String>, CustomError>>>>;

    /// ユーザー定義のエラー型 (`From<Error>` を実装する)
    #[derive(Debug)]
    struct CustomError(Error);

    impl From<Error> for CustomError {
        fn from(e: Error) -> Self {
            Self(e)
        }
    }

    /// 包んだエラーを表示できるようにする (この実装がフィールドを読む)
    impl std::fmt::Display for CustomError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "custom error: {}", self.0)
        }
    }

    /// ユーザー定義のハンドラー
    ///
    /// Video Toolbox のコールバックは別スレッドから呼ばれるため、結果は `Mutex` で保護した
    /// ベクターに積む。検証は `encode` / `finish` の完了後に同じテストスレッドで行う。
    struct CustomHandler {
        /// 受け取った結果を蓄積する
        results: Results,
    }

    impl EncodeHandler for CustomHandler {
        type UserData = String;
        type Error = CustomError;

        fn on_encoded(&mut self, result: Result<EncodedFrame<String>, Self::Error>) {
            self.results
                .lock()
                .expect("結果バッファの mutex が poison になっている")
                .push(result);
        }
    }

    let results: Results = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        minimal_encoder_config(),
        CustomHandler {
            results: Arc::clone(&results),
        },
    )?;
    let (y, u, v) = build_i420_black_frame_640x480();
    encoder.encode(
        &FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        },
        &EncodeOptions::default(),
        "custom user data".to_string(),
    )?;
    encoder.finish()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let count = results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .len();
        if count >= 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "独自ハンドラーのコールバックが届くのを待ってタイムアウトした"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let mut guard = results
        .lock()
        .expect("結果バッファの mutex が poison になっている");
    assert_eq!(guard.len(), 1, "コールバックは 1 件届くこと");
    match guard.pop().expect("コールバック結果が届いていない") {
        Ok(frame) => {
            assert_eq!(frame.user_data, "custom user data");
            assert!(!frame.data.is_empty());
        }
        Err(e) => panic!("独自ハンドラーにエラーが届いた: {e:?}"),
    }
    Ok(())
}

/// reconfigure 成功時に config が更新されることを検証する
/// (ExpectedFrameRate は単一整数のため分母が 1 に正規化される)
#[test]
fn reconfigure_updates_config_on_success() -> Result<(), Error> {
    let config = encoder_config(false);
    let mut encoder = Encoder::new(config, noop_encode_handler())?;
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(250_000),
        expected_frame_rate: Some(60),
    })?;
    assert_eq!(encoder.config().average_bitrate, Some(250_000));
    assert_eq!(encoder.config().fps_numerator, 60);
    assert_eq!(encoder.config().fps_denominator, 1);
    Ok(())
}

/// bitrate のみ更新で fps (30_000/1_001) が初期値のまま保たれることを検証する。
/// 初期 fps を分数にすることで、既定値 (1/1) への巻き戻りや分母の誤正規化を検出できる
#[test]
fn reconfigure_updates_only_average_bitrate() -> Result<(), Error> {
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

/// fps のみ更新で bitrate が初期値のまま保たれ、分母が 1 に正規化されることを検証する。
/// 初期値を既定値と区別できる値にすることで、更新時に他項目が初期値へ上書きされる回帰を検出できる
#[test]
fn reconfigure_updates_only_expected_frame_rate() -> Result<(), Error> {
    // 初期 fps を分数にすることで、正規化漏れ (分母 1_001 のまま) を検出できる。
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

/// 全項目 None の reconfigure が no-op であり、設定を変えず後続の encode が成功することを検証する
#[test]
fn reconfigure_is_noop_when_all_none() -> Result<(), Error> {
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
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;
    encoder.reconfigure(ReconfigureParams::default())?;
    assert_eq!(encoder.config().average_bitrate, before_bitrate);
    assert_eq!(encoder.config().fps_numerator, before_fps_num);
    assert_eq!(encoder.config().fps_denominator, before_fps_den);
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

/// average_bitrate に 0 を指定した reconfigure が InvalidConfig で拒否されることを検証する
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

/// expected_frame_rate に 0 を指定した reconfigure が InvalidConfig で拒否されることを検証する
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

/// expected_frame_rate が i32::MAX を超えると InvalidConfig で拒否されることを検証する
/// (CFNumber は SInt32 のため)
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

/// average_bitrate が i64::MAX を超えると InvalidConfig で拒否されることを検証する
/// (CFNumber は SInt64 のため)
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

/// expected_frame_rate と average_bitrate を同時に不正にした reconfigure が
/// expected_frame_rate のエラーを先に返すことを検証する
///
/// 構築 (Encoder::new) と再設定 (Encoder::reconfigure) で検証順序が揃っていることを
/// 固定するテスト。既存のフィールド単位の拒否テストは各エラー種別しか見ていないため、
/// 共通の検証関数の中で順序が逆転しても検出できない。
#[test]
fn reconfigure_rejects_prioritizing_fps_error_over_bitrate_error() {
    assert!(matches!(
        reconfigure_err(ReconfigureParams {
            // 両フィールドを不正値にして、どちらのエラーが優先されるかを判定する
            average_bitrate: Some(0),
            expected_frame_rate: Some(0),
        }),
        Error::InvalidConfig { field, reason }
            if field == "expected_frame_rate" && reason == "must not be zero"
    ));
}

/// Nv12 の UV プレーン長が不足していると InsufficientFrameData で拒否され、
/// コールバックが発火しないことを検証する
#[test]
fn encode_rejects_insufficient_nv12_uv_plane() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut enc = Encoder::new(
        minimal_nv12_encoder_config(),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
    assert!(
        results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .is_empty()
    );
    Ok(())
}

/// Encoder::new の構築時に bytes 0 のデータレートリミットが
/// InvalidConfig で拒否されることを検証する
#[test]
fn new_rejects_zero_bytes_data_rate_limit() {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![DataRateLimit {
        bytes: 0,
        window: Duration::from_secs(1),
    }];
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("構築時に bytes 0 のデータレートリミットが拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "bytes must not be zero"
    ));
}

/// Encoder::new の構築時に 3 個以上のデータレートリミットが InvalidConfig で
/// 拒否されることを検証する (Video Toolbox の仕様上 0〜2 個のため)
#[test]
fn new_rejects_more_than_two_data_rate_limits() {
    let limit = DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    };
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![limit; 3];
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("構築時に 3 個のデータレートリミットが拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "must contain at most two limits"
    ));
}

/// Encoder::new の構築時に bytes が i64::MAX を超えるデータレートリミットが
/// InvalidConfig で拒否されることを検証する (CFNumber は SInt64 のため)
#[test]
fn new_rejects_data_rate_limit_bytes_above_i64_max() {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![DataRateLimit {
        bytes: i64::MAX as u64 + 1,
        window: Duration::from_secs(1),
    }];
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("構築時に i64 を超えるデータレートリミットが拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "bytes must fit in i64 for CFNumber"
    ));
}

/// Encoder::new の構築時に window 0 のデータレートリミットが
/// InvalidConfig で拒否されることを検証する
#[test]
fn new_rejects_zero_window_data_rate_limit() {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::ZERO,
    }];
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("構築時に window 0 のデータレートリミットが拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "data_rate_limits" && reason == "window must not be zero"
    ));
}

/// Encoder::new の構築時に average_bitrate 0 が InvalidConfig で拒否されることを検証する
#[test]
fn new_rejects_zero_average_bitrate() {
    let mut config = minimal_encoder_config();
    config.average_bitrate = Some(0);
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("構築時に平均ビットレート 0 が拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "average_bitrate" && reason == "must not be zero"
    ));
}

/// 構築時に空 Vec を渡すと上限なしとして扱われ、config() でも空のまま返ることを検証する
#[test]
fn new_accepts_empty_data_rate_limits() -> Result<(), Error> {
    let mut config = minimal_encoder_config();
    config.data_rate_limits = Vec::new();
    let encoder = Encoder::new(config, noop_encode_handler())?;
    assert!(encoder.config().data_rate_limits.is_empty());
    Ok(())
}

/// 未対応のプロパティを指定した `Encoder::new` が、原因のプロパティ名を含むエラーを返すことを検証する
///
/// Apple Silicon の Apple エンコーダーは `kVTCompressionPropertyKey_MaxFrameDelayCount` を
/// 読み取り専用として扱い、設定すると `kVTParameterErr` (-12900) を返す
/// (macOS 26.5 / Apple M1 の実測)。未指定 (`None`) の場合はこのプロパティを設定しないため
/// 構築に成功する。このテストが失敗するようになった場合は、既定の選択で使われるエンコーダーが
/// このプロパティを受け付けるようになったことを意味するため、他の未対応プロパティを探すか
/// `max_frame_delay_count` の扱いを見直すこと。
#[test]
fn encoder_reports_property_rejected_by_video_toolbox() -> Result<(), Error> {
    // 未指定ならプロパティを設定しないため、読み取り専用のプロパティでも構築できる
    let mut config = minimal_encoder_config();
    config.max_frame_delay_count = None;
    Encoder::new(config, noop_encode_handler())?;

    // 明示指定すると Video Toolbox が受け付けず、どのプロパティが原因かがエラーから分かる
    let mut config = minimal_encoder_config();
    config.max_frame_delay_count = NonZeroU32::new(2);
    let err = Encoder::new(config, noop_encode_handler())
        .map(|_| ())
        .expect_err("未対応のプロパティを指定した構築は失敗すること");
    match err {
        Error::VideoToolbox {
            status,
            function,
            property,
        } => {
            assert_eq!(
                function, "VTSessionSetProperty",
                "失敗した関数は VTSessionSetProperty であること"
            );
            assert_eq!(
                property.as_deref(),
                Some("kVTCompressionPropertyKey_MaxFrameDelayCount"),
                "受け付けられなかったプロパティ名が入ること"
            );
            assert_ne!(status, 0, "失敗したステータスコード ({status}) が入ること");
        }
        other => panic!("VideoToolbox エラーを期待したが、実際は: {other}"),
    }
    Ok(())
}

/// Encoder::new 直後の config() が入力した全フィールドを変更なしで返すことを検証する。
/// 既定値と区別できる値を使うことで、ハードコードされた既定値を返す回帰を検出する
#[test]
fn encoder_config_returns_initial_value() -> Result<(), Error> {
    let mut config = encoder_config(false);
    config.fps_numerator = 30;
    config.real_time = Some(true);
    config.prioritize_encoding_speed_over_quality = Some(true);
    config.maximize_power_efficiency = Some(true);
    config.allow_frame_reordering = Some(true);
    config.max_key_frame_interval = std::num::NonZeroU32::new(60);
    config.max_key_frame_interval_duration = Some(Duration::from_secs(2));
    // max_frame_delay_count は Apple Silicon の Apple エンコーダーが受け付けないため指定せず、
    // 未指定のまま保持されることを確認する (`encoder_reports_property_rejected_by_video_toolbox` を参照)
    config.max_frame_delay_count = None;
    config.data_rate_limits = vec![DataRateLimit {
        bytes: 93_750,
        window: Duration::from_secs(1),
    }];
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
    assert_eq!(
        got.max_frame_delay_count, None,
        "未指定の max_frame_delay_count は None のまま保持されること"
    );
    assert_eq!(got.data_rate_limits, config.data_rate_limits);
    // codec はバリアントと中身を確認する
    match (&got.codec, &config.codec) {
        (CodecConfig::H264(a), CodecConfig::H264(b)) => {
            assert_eq!(a.profile, b.profile);
            assert_eq!(a.entropy_mode, b.entropy_mode);
        }
        _ => panic!("コーデックのバリアントが一致しない"),
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
    config.real_time = Some(true);
    config.prioritize_encoding_speed_over_quality = Some(true);
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
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
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
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
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
            "ウィンドウ {i} が {bytes} バイトを生成し、ハードリミット {LIMIT_BYTES_PER_SEC} (+50%) を超えた"
        );
    }
    // レート制御で出力が崩壊していないこと (2 Mbps 要求 + ノイズ帯なら上限の 1/4 は確実に使う)
    let total: u64 = sizes.iter().sum();
    assert!(
        total >= LIMIT_BYTES_PER_SEC * (FRAMES as u64 / FPS as u64) / 4,
        "エンコーダー出力が崩壊した: {FRAMES} フレームで合計 {total} バイト"
    );
    Ok(())
}

/// H.264 でデータレートリミットが 1 秒ウィンドウの出力バイト数を制限することを検証する
#[test]
fn data_rate_limits_cap_windowed_output_h264() -> Result<(), Error> {
    data_rate_limits_cap_windowed_output(false)
}

/// H.265 でデータレートリミットが 1 秒ウィンドウの出力バイト数を制限することを検証する
#[test]
fn data_rate_limits_cap_windowed_output_h265() -> Result<(), Error> {
    data_rate_limits_cap_windowed_output(true)
}

/// 動的更新の検証に使う低ビットレート (200 kbps)
const LOW_BITRATE: u64 = 200_000;

/// 動的更新の検証に使う高ビットレート (8 Mbps)
const HIGH_BITRATE: u64 = 8_000_000;

/// 出力バイト数を集計するウィンドウのフレーム数 (30 fps のとき 1 秒ぶん)
const WINDOW_FRAMES: usize = 30;

/// ビットレート目標に追従して出力サイズが変わる合成フレームを生成する
///
/// 全面グラデーション + 下部 1/4 の低振幅ノイズで構成する。低振幅ノイズは量子化で
/// 落とせるため、目標ビットレートを下げると詳細を捨てて出力が小さくなり、上げると
/// 詳細を残して出力が大きくなる (実測で 200 kbps と 8 Mbps の間に約 40 倍の差が出る)。
/// `synthetic_i420_frame` の高振幅ノイズは最低品質でも目標に収まらず、ビットレート変更の
/// 反映を出力サイズから検出できないため、動的更新の検証にはこちらを使う。
fn bitrate_sensitive_i420_frame(frame_index: usize, seed: &mut u64) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
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
        let noise = ((*seed >> 40) as i16 & 0x0f) - 8;
        *b = (*b as i16 + noise).clamp(0, 255) as u8;
    }
    (y_plane, vec![128u8; SIZE / 4], vec![128u8; SIZE / 4])
}

/// ビットレートに追従する合成フレームを `frames` 枚エンコードし、
/// [`WINDOW_FRAMES`] フレームごとの出力バイト数合計を返す
///
/// `reconfigure_at` に並べたフレーム番号の直前で `reconfigure` を呼ぶ。フレーム番号は
/// 投入順のインデックスであり、`allow_frame_reordering: false` のため出力順と一致する
/// (Video Toolbox の保証ではなく実測前提)。
fn windowed_output_bytes(
    config: EncoderConfig,
    reconfigure_at: &[(usize, ReconfigureParams)],
    frames: usize,
) -> Result<Vec<u64>, Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        config,
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let mut seed = 0x5eed_5eed_5eed_5eedu64;
    for i in 0..frames {
        for (at, params) in reconfigure_at {
            if *at == i {
                encoder.reconfigure(params.clone())?;
            }
        }
        let (y, u, v) = bitrate_sensitive_i420_frame(i, &mut seed);
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

    let callbacks = wait_and_take_results(&results, frames);
    let mut sizes = Vec::new();
    for callback in callbacks {
        match callback {
            Ok(frame) => sizes.push(frame.data.len() as u64),
            Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
        }
    }
    assert_eq!(sizes.len(), frames, "全フレームのエンコード結果が返ること");

    let windows: Vec<u64> = sizes
        .chunks(WINDOW_FRAMES)
        .map(|chunk| chunk.iter().sum())
        .collect();
    eprintln!("ウィンドウごとの出力バイト数: {windows:?}");
    Ok(windows)
}

/// `reconfigure` の `average_bitrate` 変更がエンコード中の出力レートに反映されることを検証する
///
/// 200 kbps で 30 フレーム → 8 Mbps へ変更して 60 フレーム → 200 kbps へ戻して 30 フレーム
/// エンコードし、30 フレームごとの出力バイト数がビットレートの変更に追従することを確認する。
/// Video Toolbox がエンコード中の `AverageBitRate` 変更を受け付けなくなると、この assert が
/// 落ちる (動的更新の実効性を出力から検出するための回帰テスト)。
#[test]
fn reconfigure_average_bitrate_changes_mid_stream_output_rate() -> Result<(), Error> {
    // 200 kbps = 1 秒あたり 25,000 バイト。実測は 25〜28 kB のため 4 倍の余裕を取る
    const LOW_MAX: u64 = 100_000;
    // 8 Mbps = 1 秒あたり 1,000,000 バイト。実測は約 1 MB のため半分以下の閾値を取る
    const HIGH_MIN: u64 = 400_000;

    let mut config = encoder_config(false);
    config.average_bitrate = Some(LOW_BITRATE);
    config.fps_numerator = WINDOW_FRAMES as u32;
    config.fps_denominator = 1;

    // 最初のウィンドウはキーフレームを含むが、ビットレート目標は最初から低いため判定に使える
    let windows = windowed_output_bytes(
        config,
        &[
            (
                WINDOW_FRAMES,
                ReconfigureParams {
                    average_bitrate: Some(HIGH_BITRATE),
                    ..Default::default()
                },
            ),
            (
                WINDOW_FRAMES * 3,
                ReconfigureParams {
                    average_bitrate: Some(LOW_BITRATE),
                    ..Default::default()
                },
            ),
        ],
        WINDOW_FRAMES * 4,
    )?;
    assert_eq!(windows.len(), 4);

    let low_before = windows[0];
    assert!(
        low_before < LOW_MAX,
        "8 Mbps へ変更する前の出力が 200 kbps 相当より大きい: {low_before} バイト"
    );
    for (i, high) in windows[1..3].iter().enumerate() {
        assert!(
            *high > HIGH_MIN,
            "8 Mbps へ変更した後の出力が反映されていない (ウィンドウ {}): {high} バイト",
            i + 1
        );
    }
    let low_after = windows[3];
    assert!(
        low_after < LOW_MAX,
        "200 kbps へ戻した後の出力が反映されていない: {low_after} バイト"
    );
    Ok(())
}

/// `reconfigure` の `expected_frame_rate` 変更がエンコード中の出力レートに反映されることを検証する
///
/// 8 Mbps / 30 fps で 30 フレームエンコードしてから 60 fps へ変更し、さらに 90 フレーム
/// エンコードする。`ExpectedFrameRate` が反映されると 1 フレームあたりの目標ビット量が
/// 半分になるため、30 フレーム (30 fps では 1 秒、60 fps では 0.5 秒) ごとの合計バイト数は
/// 約半分になる。Video Toolbox がエンコード中の `ExpectedFrameRate` 変更を受け付けなくなると
/// この assert が落ちる (動的更新の実効性を出力から検出するための回帰テスト)。
#[test]
fn reconfigure_expected_frame_rate_changes_mid_stream_output_rate() -> Result<(), Error> {
    let mut config = encoder_config(false);
    config.average_bitrate = Some(HIGH_BITRATE);
    config.fps_numerator = WINDOW_FRAMES as u32;
    config.fps_denominator = 1;

    let windows = windowed_output_bytes(
        config,
        &[(
            WINDOW_FRAMES,
            ReconfigureParams {
                expected_frame_rate: Some(2 * WINDOW_FRAMES as u32),
                ..Default::default()
            },
        )],
        WINDOW_FRAMES * 4,
    )?;
    assert_eq!(windows.len(), 4);

    let at_30fps = windows[0];
    assert!(
        at_30fps > 700_000,
        "8 Mbps / 30 fps の出力が目標より小さい: {at_30fps} バイト"
    );
    // 遷移中のウィンドウ (windows[1]) はレート制御の追従に時間がかかるため判定に使わない
    let at_60fps = windows[3];
    assert!(
        at_60fps * 4 < at_30fps * 3,
        "60 fps へ変更した後の出力が半分になっていない: {at_60fps} バイト (30 fps 時 {at_30fps} バイト)"
    );
    Ok(())
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
    // panic メッセージはテストコード出自のため日本語 (ライブラリのログフォーマットは英語のまま)
    helpers::assert_log_contains(&log, "テストハンドラが意図的に panic した");
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
                    panic!("テストハンドラが意図的に panic した");
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

/// 統計値がエンコードの進行に応じて増え、完了後に `in_flight_frames` が 0 に戻ることを検証する
///
/// `Encoder::stats` の参照はエンコーダーと共有されているため、構築直後・送信後・
/// 出力コールバック到着後の各時点で同じ参照を読んで値を確認する。
#[test]
fn encoder_stats_counts_encoded_frames_and_outputs() -> Result<(), Error> {
    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<EncodedFrame<u64>, Error>| {
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    // 構築直後の統計値はすべて 0 である
    let stats = encoder.stats();
    assert_eq!(stats.total_encode_count.get(), 0);
    assert_eq!(stats.total_output_frame_count.get(), 0);
    assert_eq!(stats.total_error_count.get(), 0);
    assert_eq!(stats.total_reconfigure_count.get(), 0);
    assert_eq!(stats.in_flight_frames.get(), 0);

    // 3 フレームを送信し、すべての出力が届くまで待つ
    let (y, u, v) = build_i420_black_frame();
    for user_data in 0..3u64 {
        encoder.encode(
            &FrameData::I420 {
                y: &y,
                u: &u,
                v: &v,
            },
            &EncodeOptions::default(),
            user_data,
        )?;
    }
    encoder.finish()?;

    let callbacks = wait_and_take_results(&results, 3);
    assert_eq!(callbacks.len(), 3, "エンコード結果が 3 件届くこと");
    for callback in &callbacks {
        assert!(
            callback.is_ok(),
            "正常な入力のエンコードは成功すること: {callback:?}"
        );
    }

    // 統計値の計上はハンドラーの実行前に行われるため、結果の到着後は計上済みである
    let stats = encoder.stats();
    assert_eq!(
        stats.total_encode_count.get(),
        3,
        "送信したフレーム数だけ total_encode_count が増えること"
    );
    assert_eq!(
        stats.total_output_frame_count.get(),
        3,
        "出力できたフレーム数だけ total_output_frame_count が増えること"
    );
    assert_eq!(
        stats.total_error_count.get(),
        0,
        "正常な入力ではエラーが計上されないこと"
    );
    assert_eq!(
        stats.in_flight_frames.get(),
        0,
        "すべての出力コールバックの到着後に in_flight_frames が 0 に戻ること"
    );

    Ok(())
}

/// `in_flight_frames` が送信で増え、出力コールバックの到着で減ることを検証する
///
/// 1 件目のコールバックをハンドラー内でブロックし、Video Toolbox のコールバックが
/// 直列に配信される性質を利用して後続フレームのコールバックを保留させる。
/// 保留中に送信したフレームはまだユーザーデータを回収されていないため、
/// その件数が `in_flight_frames` に現れる。
#[test]
fn encoder_stats_in_flight_frames_tracks_pending_callbacks() -> Result<(), Error> {
    let (started_tx, started_rx) = channel::<()>();
    let (release_tx, release_rx) = channel::<()>();

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        encoder_config(false),
        FnEncodeHandler::new({
            let results = Arc::clone(&results);
            // コールバックは Video Toolbox のコールバックスレッドから呼ばれるため、
            // 1 件目かどうかの判定はアトミックに行う
            let is_first = AtomicBool::new(true);
            move |result: Result<EncodedFrame<u64>, Error>| {
                // 1 件目のコールバックだけ、テスト本体が解放するまでブロックする
                if is_first.swap(false, Ordering::Relaxed) {
                    started_tx
                        .send(())
                        .expect("コールバック開始の通知に失敗した");
                    release_rx.recv().expect("コールバックの解放待ちに失敗した");
                }
                results
                    .lock()
                    .expect("結果バッファの mutex が poison になっている")
                    .push(result);
            }
        }),
    )?;

    let (y, u, v) = build_i420_black_frame();
    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };

    // 1 件目を送信し、そのコールバックがハンドラーに入ってブロックするまで待つ。
    // この時点で 1 件目のユーザーデータは回収済みなので in-flight は 0 である。
    encoder.encode(&frame, &EncodeOptions::default(), 0)?;
    started_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("1 件目のエンコードコールバックが開始されること");
    assert_eq!(
        encoder.stats().in_flight_frames.get(),
        0,
        "ユーザーデータを回収したフレームは in-flight に数えないこと"
    );

    // コールバックがブロックされている間に 3 件を送信する。これらのコールバックは
    // 直列な配信キューで保留されるため、いずれも in-flight として観測できる。
    for user_data in 1..4u64 {
        encoder.encode(&frame, &EncodeOptions::default(), user_data)?;
    }
    assert_eq!(
        encoder.stats().in_flight_frames.get(),
        3,
        "送信済みでコールバック未到着のフレーム数が in_flight_frames に現れること"
    );

    // ブロックを解放して全フレームの出力を待つ
    release_tx.send(()).expect("コールバックの解放に失敗した");
    encoder.finish()?;
    let callbacks = wait_and_take_results(&results, 4);
    assert_eq!(callbacks.len(), 4, "エンコード結果が 4 件届くこと");

    let stats = encoder.stats();
    assert_eq!(stats.total_encode_count.get(), 4);
    assert_eq!(stats.total_output_frame_count.get(), 4);
    assert_eq!(stats.total_error_count.get(), 0);
    assert_eq!(
        stats.in_flight_frames.get(),
        0,
        "全フレームの出力後に in_flight_frames が 0 に戻ること"
    );

    Ok(())
}

/// `total_reconfigure_count` が成功した `reconfigure` だけで増えることを検証する
///
/// 更新対象が無い no-op と、検証で拒否された更新は Video Toolbox に到達しないため
/// 計上しない。構築時に `EncoderStats` が 0 で初期化されることも合わせて確認する。
#[test]
fn encoder_stats_counts_successful_reconfigure_only() -> Result<(), Error> {
    let mut encoder = Encoder::new(minimal_encoder_config(), noop_encode_handler())?;
    assert_eq!(
        encoder.stats().total_reconfigure_count.get(),
        0,
        "構築直後の total_reconfigure_count は 0 であること"
    );

    // 全項目 None は no-op であり、VTSessionSetProperties を呼ばない
    encoder.reconfigure(ReconfigureParams::default())?;
    assert_eq!(
        encoder.stats().total_reconfigure_count.get(),
        0,
        "no-op の reconfigure は計上しないこと"
    );

    // 有効な更新は VTSessionSetProperties に成功する
    encoder.reconfigure(ReconfigureParams {
        average_bitrate: Some(1_000_000),
        ..Default::default()
    })?;
    assert_eq!(
        encoder.stats().total_reconfigure_count.get(),
        1,
        "成功した reconfigure が 1 回計上されること"
    );

    // 検証で拒否された更新は self に触れる前にエラーになる
    let err = encoder
        .reconfigure(ReconfigureParams {
            average_bitrate: Some(0),
            ..Default::default()
        })
        .expect_err("average_bitrate 0 は拒否されること");
    assert!(matches!(err, Error::InvalidConfig { .. }));
    assert_eq!(
        encoder.stats().total_reconfigure_count.get(),
        1,
        "拒否された reconfigure は計上しないこと"
    );

    Ok(())
}
