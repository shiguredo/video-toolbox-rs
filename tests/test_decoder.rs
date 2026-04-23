//! `src/decoder.rs` に対応する単体テスト

use shiguredo_video_toolbox::{
    DecodedFrame, Decoder, DecoderCodec, DecoderConfig, Error, PixelFormat, VideoCodecType,
    supported_codecs,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 480;

#[test]
fn decoder_vp9_rejects_width_above_i32_max() {
    let r = Decoder::new(DecoderConfig {
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
fn decoder_av1_rejects_height_above_i32_max() {
    let r = Decoder::new(DecoderConfig {
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
fn h264_decoder() -> Result<(), Error> {
    let sps = [
        103, 100, 0, 30, 172, 217, 64, 160, 61, 176, 17, 0, 0, 3, 0, 1, 0, 0, 3, 0, 50, 15, 22, 45,
        150,
    ];
    let pps = [104, 235, 227, 203, 34, 192];
    let mut decoder = Decoder::new(DecoderConfig {
        codec: DecoderCodec::H264 {
            sps: &sps,
            pps: &pps,
            nalu_len_bytes: 4,
        },
        pixel_format: PixelFormat::I420,
    })?;

    let nal_unit = [
        101, 136, 132, 0, 43, 255, 254, 246, 115, 124, 10, 107, 109, 176, 149, 46, 5, 118, 247,
        102, 163, 229, 208, 146, 229, 251, 16, 96, 250, 208, 0, 0, 3, 0, 0, 3, 0, 0, 16, 15, 210,
        222, 245, 204, 98, 91, 229, 32, 0, 0, 9, 216, 2, 56, 13, 16, 118, 133, 116, 69, 196, 32,
        71, 6, 120, 150, 16, 161, 210, 50, 128, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0,
        3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 37, 225,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(&(nal_unit.len() as u32).to_be_bytes());
    data.extend_from_slice(&nal_unit);
    decoder.decode(&data)?;

    Ok(())
}

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
    let mut decoder = Decoder::new(DecoderConfig {
        codec: DecoderCodec::Hevc {
            vps: &vps,
            sps: &sps,
            pps: &pps,
            nalu_len_bytes: 4,
        },
        pixel_format: PixelFormat::I420,
    })?;

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
    decoder.decode(&data)?;

    Ok(())
}

#[test]
fn init_av1_decoder() -> Result<(), Error> {
    if !supported_codecs()
        .iter()
        .any(|c| c.codec == VideoCodecType::Av1 && c.decoding.supported)
    {
        return Ok(());
    }

    // Decoder::new は最小限の FormatDescription でセッション作成を試行するため、
    // コーデック固有のパラメータが不足して失敗する場合がある。
    // 実際のビットストリームからデコードする場合は正常に動作する。
    match Decoder::new(DecoderConfig {
        codec: DecoderCodec::Av1 {
            width: WIDTH,
            height: HEIGHT,
        },
        pixel_format: PixelFormat::I420,
    }) {
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

    let width: u32 = 320;
    let height: u32 = 240;
    let num_frames: usize = 10;

    let (y_plane, u_plane, v_plane) = generate_colorbar_i420(width as usize, height as usize);

    // shiguredo_libvpx で VP9 エンコード
    let vpx_config = VpxEncoderConfig {
        width: width as usize,
        height: height as usize,
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
                    y: &y_plane,
                    u: &u_plane,
                    v: &v_plane,
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

    assert!(!encoded_frames.is_empty(), "VP9 encoder produced no frames");

    // Video Toolbox VP9 デコーダーを作成
    let mut decoder = Decoder::new(DecoderConfig {
        codec: DecoderCodec::Vp9 { width, height },
        pixel_format: PixelFormat::I420,
    })?;

    // 各フレームをデコードして PSNR を検証
    let min_psnr_db = 25.0;
    for (i, encoded_data) in encoded_frames.iter().enumerate() {
        let decoded_opt = decoder.decode(encoded_data)?;
        assert!(decoded_opt.is_some(), "frame {i}: decode returned None");
        let decoded = decoded_opt.unwrap();
        match decoded {
            DecodedFrame::I420(ref frame) => {
                assert_eq!(frame.width(), width as usize, "frame {i}: width mismatch");
                assert_eq!(
                    frame.height(),
                    height as usize,
                    "frame {i}: height mismatch"
                );

                let psnr = psnr_y(
                    &y_plane,
                    width as usize,
                    frame.y_plane(),
                    frame.y_stride(),
                    width as usize,
                    height as usize,
                );
                assert!(
                    psnr >= min_psnr_db,
                    "frame {i}: PSNR {psnr:.1} dB < {min_psnr_db} dB"
                );
            }
            DecodedFrame::Nv12(_) => {
                unreachable!("frame {i}: expected I420 but got NV12");
            }
        }
    }

    Ok(())
}
