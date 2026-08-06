//! `src/decoder.rs` に対応する単体テスト

mod helpers;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use shiguredo_video_toolbox::{
    DecodedFrame, Decoder, DecoderCodec, DecoderConfig, Error, FnDecodeHandler, PixelFormat,
    VideoCodecType, supported_codecs,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

/// H.264 デコードテスト用の SPS パラメータセット
const H264_SPS: &[u8] = &[
    103, 100, 0, 30, 172, 217, 64, 160, 61, 176, 17, 0, 0, 3, 0, 1, 0, 0, 3, 0, 50, 15, 22, 45, 150,
];

/// H.264 デコードテスト用の PPS パラメータセット
const H264_PPS: &[u8] = &[104, 235, 227, 203, 34, 192];

/// H.264 デコードテスト用の NAL ユニット (I フレーム 1 枚分)
const H264_NAL_UNIT: &[u8] = &[
    101, 136, 132, 0, 43, 255, 254, 246, 115, 124, 10, 107, 109, 176, 149, 46, 5, 118, 247, 102,
    163, 229, 208, 146, 229, 251, 16, 96, 250, 208, 0, 0, 3, 0, 0, 3, 0, 0, 16, 15, 210, 222, 245,
    204, 98, 91, 229, 32, 0, 0, 9, 216, 2, 56, 13, 16, 118, 133, 116, 69, 196, 32, 71, 6, 120, 150,
    16, 161, 210, 50, 128, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3,
    0, 0, 3, 0, 37, 225,
];

enum DecodeEvent {
    I420 {
        user_data: u64,
        width: usize,
        height: usize,
        y_plane: Vec<u8>,
        y_stride: usize,
    },
    Nv12 {
        user_data: u64,
    },
    Err(Error),
}

type SharedDecodeResults = Arc<Mutex<Vec<DecodeEvent>>>;

/// H.264 の長さプレフィクス (4 バイト) 付き NAL ユニットのデータを構築する
fn h264_nalu_data() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(H264_NAL_UNIT.len() as u32).to_be_bytes());
    data.extend_from_slice(H264_NAL_UNIT);
    data
}

fn push_decode_event(results: &SharedDecodeResults, result: Result<DecodedFrame<u64>, Error>) {
    let event = match result {
        Ok(DecodedFrame::I420 { frame, user_data }) => DecodeEvent::I420 {
            user_data,
            width: frame.width(),
            height: frame.height(),
            y_plane: frame.y_plane().to_vec(),
            y_stride: frame.y_stride(),
        },
        Ok(DecodedFrame::Nv12 { user_data, .. }) => DecodeEvent::Nv12 { user_data },
        Err(e) => DecodeEvent::Err(e),
    };
    results
        .lock()
        .expect("結果バッファの mutex が poison になっている")
        .push(event);
}

fn take_results(results: &SharedDecodeResults) -> Vec<DecodeEvent> {
    let mut guard = results
        .lock()
        .expect("結果バッファの mutex が poison になっている");
    std::mem::take(&mut *guard)
}

/// VP9 デコーダーの構築で width が i32::MAX を超えると InvalidConfig で拒否されることを検証する
#[test]
fn decoder_vp9_rejects_width_above_i32_max() {
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Vp9 {
                width: i32::MAX as u32 + 1,
                height: 480,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field, .. }) if field == "width"
    ));
}

/// AV1 デコーダーの構築で height が i32::MAX を超えると InvalidConfig で拒否されることを検証する
#[test]
fn decoder_av1_rejects_height_above_i32_max() {
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Av1 {
                width: 640,
                height: i32::MAX as u32 + 1,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field, .. }) if field == "height"
    ));
}

/// ハードコードされた H.264 ビットストリーム (SPS / PPS / IDR フレーム) をデコードし、
/// 1 フレームが出力されることを検証する。ビットストリームは 640x480 の I420 フレーム 1 枚分で、
/// ファイル内定数の H264_SPS / H264_PPS / H264_NAL_UNIT を使用する
#[test]
fn h264_decoder() -> Result<(), Error> {
    let results: SharedDecodeResults = Arc::new(Mutex::new(Vec::new()));
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: H264_SPS,
                pps: H264_PPS,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<DecodedFrame<u64>, Error>| {
                push_decode_event(&results, result);
            }
        }),
    )?;

    let data = h264_nalu_data();
    decoder.decode(&data, 7)?;
    decoder.finish()?;

    let callbacks = take_results(&results);
    assert_eq!(callbacks.len(), 1);
    match callbacks
        .into_iter()
        .next()
        .expect("コールバック結果が届いていない")
    {
        DecodeEvent::I420 {
            user_data,
            width,
            height,
            ..
        } => {
            assert_eq!(user_data, 7);
            assert_eq!(width, WIDTH as usize);
            assert_eq!(height, HEIGHT as usize);
        }
        DecodeEvent::Nv12 { .. } => {
            unreachable!("I420 を期待したが NV12 が届いた");
        }
        DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
    }

    Ok(())
}

/// ハードコードされた H.265 ビットストリーム (VPS / SPS / PPS / IDR フレーム) をデコードし、
/// 1 フレームが出力されることを検証する。ビットストリームは 640x480 の I420 フレーム 1 枚分で、
/// 関数内ローカルの vps / sps / pps / nal_unit 配列を使用する (nalu_len_bytes: 4)
#[test]
fn h265_decoder() -> Result<(), Error> {
    let vps = [
        64, 1, 12, 1, 255, 255, 1, 96, 0, 0, 3, 0, 144, 0, 0, 3, 0, 0, 3, 0, 90, 149, 152, 9,
    ];
    let sps = [
        66, 1, 1, 1, 96, 0, 0, 3, 0, 144, 0, 0, 3, 0, 0, 3, 0, 90, 160, 5, 2, 1, 225, 101, 149,
        154, 73, 50, 188, 5, 160, 32, 0, 0, 3, 0, 32, 0, 0, 3, 3, 33,
    ];
    let pps = [68, 1, 193, 114, 180, 98, 64];
    let results: SharedDecodeResults = Arc::new(Mutex::new(Vec::new()));
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Hevc {
                vps: &vps,
                sps: &sps,
                pps: &pps,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<DecodedFrame<u64>, Error>| {
                push_decode_event(&results, result);
            }
        }),
    )?;

    let nal_unit = [
        40, 1, 175, 29, 16, 90, 181, 140, 90, 213, 247, 1, 91, 255, 242, 78, 254, 199, 0, 31, 209,
        50, 148, 21, 162, 38, 146, 0, 0, 3, 1, 203, 169, 113, 202, 5, 24, 129, 39, 128, 0, 0, 3, 0,
        7, 204, 147, 13, 148, 32, 0, 0, 3, 0, 0, 3, 0, 12, 24, 135, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0,
        28, 240, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 8, 104, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 104, 192, 0,
        0, 3, 0, 0, 3, 0, 0, 3, 1, 223, 0, 0, 3, 0, 9, 248,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(&(nal_unit.len() as u32).to_be_bytes());
    data.extend_from_slice(&nal_unit);
    decoder.decode(&data, 11)?;
    decoder.finish()?;

    let callbacks = take_results(&results);
    assert_eq!(callbacks.len(), 1);
    match callbacks
        .into_iter()
        .next()
        .expect("コールバック結果が届いていない")
    {
        DecodeEvent::I420 {
            user_data,
            width,
            height,
            ..
        } => {
            assert_eq!(user_data, 11);
            assert_eq!(width, WIDTH as usize);
            assert_eq!(height, HEIGHT as usize);
        }
        DecodeEvent::Nv12 { .. } => {
            unreachable!("I420 を期待したが NV12 が届いた");
        }
        DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
    }

    Ok(())
}

/// AV1 デコーダーが構築できることを検証する。非対応環境では supported_codecs() の
/// 事前検査でスキップする。ビットストリームなしの最小構築ではコーデック固有の
/// パラメータ不足で UnsupportedCodec が返り得るため、Ok と UnsupportedCodec の両方を許容する
#[test]
fn init_av1_decoder() -> Result<(), Error> {
    if !supported_codecs()
        .iter()
        .any(|c| c.codec == VideoCodecType::Av1 && c.decoding.supported)
    {
        return Ok(());
    }

    // 実際のビットストリームからデコードする場合は正常に動作する。
    match Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Av1 {
                width: WIDTH,
                height: HEIGHT,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    ) {
        Ok(_) => Ok(()),
        Err(Error::UnsupportedCodec { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// SMPTE カラーバー風の I420 フレームを生成する
///
/// 7 色の縦ストライプ（白/黄/シアン/緑/マゼンタ/赤/青）を
/// BT.601 で YUV に変換し I420 形式で返す。
fn generate_colorbar_i420(width: usize, height: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    // SMPTE カラーバーの RGB 値（白/黄/シアン/緑/マゼンタ/赤/青）
    let bars: [(u8, u8, u8); 7] = [
        (235, 235, 235), // 白
        (235, 235, 16),  // 黄
        (16, 235, 235),  // シアン
        (16, 235, 16),   // 緑
        (235, 16, 235),  // マゼンタ
        (235, 16, 16),   // 赤
        (16, 16, 235),   // 青
    ];

    let y_size = width * height;
    let uv_width = width / 2;
    let uv_height = height / 2;
    let uv_size = uv_width * uv_height;
    let mut y_plane = vec![0u8; y_size];
    let mut u_plane = vec![0u8; uv_size];
    let mut v_plane = vec![0u8; uv_size];

    for y in 0..height {
        for x in 0..width {
            let bar_index = x * 7 / width;
            let (r, g, b) = bars[bar_index];

            // BT.601 RGB -> YCbCr
            let rf = r as f64;
            let gf = g as f64;
            let bf = b as f64;
            let yv = (0.257 * rf + 0.504 * gf + 0.098 * bf + 16.0).clamp(16.0, 235.0) as u8;
            y_plane[y * width + x] = yv;

            // UV は 2x2 ブロック単位（左上ピクセルで代表する）
            if y % 2 == 0 && x % 2 == 0 {
                let u = (-0.148 * rf - 0.291 * gf + 0.439 * bf + 128.0).clamp(16.0, 240.0) as u8;
                let v = (0.439 * rf - 0.368 * gf - 0.071 * bf + 128.0).clamp(16.0, 240.0) as u8;
                let uv_row = y / 2;
                let uv_col = x / 2;
                u_plane[uv_row * uv_width + uv_col] = u;
                v_plane[uv_row * uv_width + uv_col] = v;
            }
        }
    }

    (y_plane, u_plane, v_plane)
}

/// Y プレーン同士の PSNR を計算する（dB）
///
/// デコード結果はストライドにパディングが含まれる場合があるため、
/// ストライドを指定して有効ピクセルのみを比較する。
fn psnr_y(
    original: &[u8],
    original_stride: usize,
    decoded: &[u8],
    decoded_stride: usize,
    width: usize,
    height: usize,
) -> f64 {
    let mut mse_sum: f64 = 0.0;
    let pixel_count = width * height;
    for y in 0..height {
        for x in 0..width {
            let orig = original[y * original_stride + x] as f64;
            let dec = decoded[y * decoded_stride + x] as f64;
            let diff = orig - dec;
            mse_sum += diff * diff;
        }
    }
    let mse = mse_sum / pixel_count as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0_f64 * 255.0 / mse).log10()
}

/// shiguredo_libvpx の VP9 エンコーダーで生成したフレームをデコードし、
/// Y プレーンの PSNR が下限を満たすことを検証する (対応環境でない場合はスキップ)
#[test]
fn vp9_decoder() -> Result<(), Error> {
    if !supported_codecs()
        .iter()
        .any(|c| c.codec == VideoCodecType::Vp9 && c.decoding.supported)
    {
        return Ok(());
    }

    use shiguredo_libvpx::{
        CodecConfig as VpxCodecConfig, EncodeOptions as VpxEncodeOptions, Encoder as VpxEncoder,
        EncoderConfig as VpxEncoderConfig, EncodingDeadline as VpxEncodingDeadline,
        ImageData as VpxImageData, ImageFormat as VpxImageFormat,
        RateControlMode as VpxRateControlMode, Vp9Config as VpxVp9Config,
    };

    let frame_width: u32 = 320;
    let frame_height: u32 = 240;
    let num_frames: usize = 10;

    let (source_y_plane, source_u_plane, source_v_plane) =
        generate_colorbar_i420(frame_width as usize, frame_height as usize);

    // shiguredo_libvpx で VP9 エンコード
    let vpx_config = VpxEncoderConfig {
        width: frame_width as usize,
        height: frame_height as usize,
        image_format: VpxImageFormat::I420,
        fps_numerator: 30,
        fps_denominator: 1,
        target_bitrate: 1_000_000,
        min_quantizer: 0,
        max_quantizer: 63,
        cq_level: 10,
        cpu_used: Some(8),
        deadline: VpxEncodingDeadline::Realtime,
        rate_control: VpxRateControlMode::Cbr,
        lag_in_frames: None,
        threads: std::num::NonZeroUsize::new(1),
        error_resilient: false,
        keyframe_interval: std::num::NonZeroUsize::new(30),
        frame_drop_threshold: None,
        codec: VpxCodecConfig::Vp9(VpxVp9Config::default()),
    };
    let mut vpx_encoder = match VpxEncoder::new(vpx_config) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut encoded_frames: Vec<Vec<u8>> = Vec::new();
    for i in 0..num_frames {
        if vpx_encoder
            .encode(
                &VpxImageData::I420 {
                    y: &source_y_plane,
                    u: &source_u_plane,
                    v: &source_v_plane,
                },
                &VpxEncodeOptions {
                    force_keyframe: i == 0,
                },
            )
            .is_err()
        {
            return Ok(());
        }
        while let Some(frame) = vpx_encoder.next_frame() {
            encoded_frames.push(frame.data().to_vec());
        }
    }
    if vpx_encoder.finish().is_err() {
        return Ok(());
    }
    while let Some(frame) = vpx_encoder.next_frame() {
        encoded_frames.push(frame.data().to_vec());
    }

    assert!(
        !encoded_frames.is_empty(),
        "VP9 エンコーダーがフレームを生成しなかった"
    );

    let results: SharedDecodeResults = Arc::new(Mutex::new(Vec::new()));
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Vp9 {
                width: frame_width,
                height: frame_height,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new({
            let results = Arc::clone(&results);
            move |result: Result<DecodedFrame<u64>, Error>| {
                push_decode_event(&results, result);
            }
        }),
    )?;

    for (i, encoded_data) in encoded_frames.iter().enumerate() {
        decoder.decode(encoded_data, i as u64)?;
    }
    decoder.finish()?;

    let callbacks = take_results(&results);
    assert_eq!(callbacks.len(), encoded_frames.len());

    let min_psnr_db = 25.0;
    let mut seen = vec![false; encoded_frames.len()];
    for callback in callbacks {
        match callback {
            DecodeEvent::I420 {
                user_data,
                width: decoded_width,
                height: decoded_height,
                y_plane: decoded_y_plane,
                y_stride,
            } => {
                let i = user_data as usize;
                assert!(
                    i < encoded_frames.len(),
                    "フレーム番号 {user_data} が範囲外"
                );
                assert!(
                    !seen[i],
                    "コールバック user_data が重複している: {user_data}"
                );
                seen[i] = true;

                assert_eq!(
                    decoded_width, frame_width as usize,
                    "フレーム {i}: 幅が一致しない"
                );
                assert_eq!(
                    decoded_height, frame_height as usize,
                    "フレーム {i}: 高さが一致しない"
                );

                let psnr = psnr_y(
                    &source_y_plane,
                    frame_width as usize,
                    &decoded_y_plane,
                    y_stride,
                    frame_width as usize,
                    frame_height as usize,
                );
                assert!(
                    psnr >= min_psnr_db,
                    "フレーム {i}: PSNR {psnr:.1} dB が下限 {min_psnr_db} dB 未満"
                );
            }
            DecodeEvent::Nv12 { user_data, .. } => {
                unreachable!("フレーム {user_data}: I420 を期待したが NV12 が届いた");
            }
            DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
        }
    }
    assert!(seen.iter().all(|v| *v), "一部のコールバックが届いていない");

    Ok(())
}

/// ユーザーハンドラが 1 回目に panic してもプロセスが abort せず、
/// panic 捕捉後にセッションが継続して後続フレームが届くことを確認する
///
/// 1 回目のコールバックで panic し、2 回目 (user_data = 2) だけが届くことを期待している。
/// 本テストは I フレームのみを投入するため表示順 = 投入順でコールバックされ、同一の
/// IDR フレームを 2 回 decode すると Video Toolbox は各 decode につき 1 フレーム出力する
/// (いずれも実測前提で、Video Toolbox のドキュメント保証ではない)。
#[test]
fn handler_panic_is_caught_and_decode_continues() -> Result<(), Error> {
    // コールバックは Video Toolbox の別スレッドで実行されるため、グローバル subscriber で
    // ログを収集する (with_default はスレッドローカルで届かない)
    let logs = helpers::init_global_log_collector();
    helpers::clear_logs(&logs);

    decode_with_panicking_handler()?;

    let log = helpers::take_logs(&logs);
    helpers::assert_log_contains(&log, "output_callback: user handler panicked");
    // panic メッセージはテストコード出自のため日本語 (ライブラリのログフォーマットは英語のまま)
    helpers::assert_log_contains(&log, "テストハンドラが意図的に panic した");
    Ok(())
}

fn decode_with_panicking_handler() -> Result<(), Error> {
    let results: SharedDecodeResults = Arc::new(Mutex::new(Vec::new()));
    let panicked = Arc::new(AtomicBool::new(false));
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: H264_SPS,
                pps: H264_PPS,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new({
            let results = Arc::clone(&results);
            let panicked = Arc::clone(&panicked);
            move |result: Result<DecodedFrame<u64>, Error>| {
                // 1 回目のコールバックだけ panic して、後続は正常に結果を返す
                if !panicked.swap(true, Ordering::Relaxed) {
                    panic!("テストハンドラが意図的に panic した");
                }
                push_decode_event(&results, result);
            }
        }),
    )?;

    let data = h264_nalu_data();
    decoder.decode(&data, 1)?;
    decoder.decode(&data, 2)?;
    decoder.finish()?;

    let callbacks = take_results(&results);
    assert_eq!(callbacks.len(), 1);
    match callbacks
        .into_iter()
        .next()
        .expect("コールバック結果が届いていない")
    {
        DecodeEvent::I420 { user_data, .. } => assert_eq!(user_data, 2),
        DecodeEvent::Nv12 { .. } => unreachable!("I420 を期待したが NV12 が届いた"),
        DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
    }
    Ok(())
}
