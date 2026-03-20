//! raden で描画したアニメーション映像を H.264/H.265 エンコードして MP4 ファイルに保存するサンプル
//!
//! ```bash
//! cargo run --example raden_to_mp4
//! cargo run --example raden_to_mp4 -- --codec h265
//! cargo run --example raden_to_mp4 -- --codec h264 --width 1920 --height 1080 --fps 60 --duration 10
//! ```

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::num::NonZeroU32;

use raden::{Circle, CompOp, Context, Image, PipelineRuntime, PixelFormat, Rect, Rgba32};
use shiguredo_mp4::boxes::{
    Avc1Box, AvccBox, Hev1Box, HvccBox, HvccNalUintArray, SampleEntry, VisualSampleEntryFields,
};
use shiguredo_mp4::mux::{Mp4FileMuxer, Sample};
use shiguredo_mp4::{TrackKind, Uint};
use shiguredo_video_toolbox::{
    CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, FrameData, H264EncoderConfig,
    H264EntropyMode, H264Profile, HevcEncoderConfig, HevcProfile, PixelFormat as VideoPixelFormat,
};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_FPS: u32 = 30;
const DEFAULT_DURATION_SECS: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    H264,
    H265,
}

/// Prgb32 (premultiplied ARGB, リトルエンディアンで BGRA) から I420 (YUV420 planar) に変換する
fn prgb32_to_i420(
    data: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut y_plane = vec![0u8; width * height];
    let mut u_plane = vec![0u8; (width / 2) * (height / 2)];
    let mut v_plane = vec![0u8; (width / 2) * (height / 2)];

    for row in 0..height {
        for col in 0..width {
            let offset = row * stride + col * 4;
            // Prgb32 リトルエンディアン: B G R A
            let b = data[offset] as f64;
            let g = data[offset + 1] as f64;
            let r = data[offset + 2] as f64;

            // BT.601 変換
            let y = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 255.0) as u8;
            y_plane[row * width + col] = y;
        }
    }

    // U/V プレーンは 2x2 ブロック平均
    for row in 0..(height / 2) {
        for col in 0..(width / 2) {
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let offset = (row * 2 + dy) * stride + (col * 2 + dx) * 4;
                    sum_b += data[offset] as u32;
                    sum_g += data[offset + 1] as u32;
                    sum_r += data[offset + 2] as u32;
                }
            }
            let r = (sum_r / 4) as f64;
            let g = (sum_g / 4) as f64;
            let b = (sum_b / 4) as f64;

            let u = (-0.169 * r - 0.331 * g + 0.500 * b + 128.0).clamp(0.0, 255.0) as u8;
            let v = (0.500 * r - 0.419 * g - 0.081 * b + 128.0).clamp(0.0, 255.0) as u8;

            u_plane[row * (width / 2) + col] = u;
            v_plane[row * (width / 2) + col] = v;
        }
    }

    (y_plane, u_plane, v_plane)
}

/// HSV を RGB に変換する (h: 0-360, s: 0-1, v: 0-1)
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// raden でアニメーションフレームを描画する
fn render_frame(ctx: &mut Context, t: f64, w: f64, h: f64) {
    // 背景
    ctx.set_comp_op(CompOp::SrcCopy);
    let bg_hue = (t * 20.0) % 360.0;
    let (br, bg, bb) = hsv_to_rgb(bg_hue, 0.3, 0.15);
    ctx.set_fill_style(Rgba32::rgb(br, bg, bb));
    ctx.fill_all();

    ctx.set_comp_op(CompOp::SrcOver);

    // ウェーブパターン
    for wave_idx in 0..5 {
        let wi = wave_idx as f64;
        let wave_amplitude = 50.0 + wi * 20.0;
        let wave_freq = 0.008 + wi * 0.002;
        let wave_speed = 2.0 + wi * 0.3;
        let wave_y_base = h * 0.3 + wi * 80.0;

        let wave_hue = (t * 60.0 + wi * 50.0) % 360.0;
        let (wr, wg, wb) = hsv_to_rgb(wave_hue, 0.8, 0.9);
        ctx.set_fill_style(Rgba32::new(wr, wg, wb, 150));

        let mut x = 0.0;
        while x < w {
            let y =
                wave_y_base + wave_amplitude * (wave_freq * x + t * wave_speed + wi * 0.5).sin();
            let radius = 8.0 + 4.0 * (t * 3.0 + x * 0.01).sin();
            ctx.fill_circle(&Circle::new(x, y, radius));
            x += 20.0;
        }
    }

    // バウンドする円
    for ball_idx in 0..8 {
        let bi = ball_idx as f64;
        let freq_x = 0.5 + bi * 0.15;
        let freq_y = 0.7 + bi * 0.12;

        let bx = w * 0.5 + (w * 0.35) * (t * freq_x + bi * std::f64::consts::PI / 4.0).sin();
        let by = h * 0.5 + (h * 0.3) * (t * freq_y + bi * std::f64::consts::PI / 3.0).sin();
        let ball_radius = 30.0 + 15.0 * (t * 4.0 + bi).sin();

        let ball_hue = (bi * 45.0 + t * 100.0) % 360.0;
        let (cr, cg, cb) = hsv_to_rgb(ball_hue, 1.0, 1.0);
        ctx.set_fill_style(Rgba32::new(cr, cg, cb, 200));
        ctx.fill_circle(&Circle::new(bx, by, ball_radius));
    }

    // タイムスタンプ表示用の矩形
    let bar_width = (t * 50.0) % w;
    ctx.set_fill_style(Rgba32::new(255, 255, 255, 100));
    ctx.fill_rect(&Rect::new(0.0, h - 20.0, bar_width, 20.0));
}

/// H.264 用の SampleEntry を構築する
fn build_h264_sample_entry(frame: &EncodedFrame, width: u16, height: u16) -> SampleEntry {
    SampleEntry::Avc1(Avc1Box {
        visual: visual_sample_entry_fields(width, height),
        avcc_box: AvccBox {
            avc_profile_indication: frame.sps_list[0][1],
            profile_compatibility: frame.sps_list[0][2],
            avc_level_indication: frame.sps_list[0][3],
            length_size_minus_one: Uint::new(3),
            sps_list: frame.sps_list.clone(),
            pps_list: frame.pps_list.clone(),
            chroma_format: None,
            bit_depth_luma_minus8: None,
            bit_depth_chroma_minus8: None,
            sps_ext_list: vec![],
        },
        unknown_boxes: vec![],
    })
}

/// H.265 用の SampleEntry を構築する
fn build_h265_sample_entry(frame: &EncodedFrame, width: u16, height: u16) -> SampleEntry {
    // HEVC NAL unit type: VPS=32, SPS=33, PPS=34
    let mut nalu_arrays = Vec::new();
    if !frame.vps_list.is_empty() {
        nalu_arrays.push(HvccNalUintArray {
            array_completeness: Uint::new(1),
            nal_unit_type: Uint::new(32),
            nalus: frame.vps_list.clone(),
        });
    }
    if !frame.sps_list.is_empty() {
        nalu_arrays.push(HvccNalUintArray {
            array_completeness: Uint::new(1),
            nal_unit_type: Uint::new(33),
            nalus: frame.sps_list.clone(),
        });
    }
    if !frame.pps_list.is_empty() {
        nalu_arrays.push(HvccNalUintArray {
            array_completeness: Uint::new(1),
            nal_unit_type: Uint::new(34),
            nalus: frame.pps_list.clone(),
        });
    }

    SampleEntry::Hev1(Hev1Box {
        visual: visual_sample_entry_fields(width, height),
        hvcc_box: HvccBox {
            general_profile_space: Uint::new(0),
            general_tier_flag: Uint::new(0),
            general_profile_idc: Uint::new(1), // Main
            general_profile_compatibility_flags: 0,
            general_constraint_indicator_flags: Uint::new(0),
            general_level_idc: 0,
            min_spatial_segmentation_idc: Uint::new(0),
            parallelism_type: Uint::new(0),
            chroma_format_idc: Uint::new(1), // 4:2:0
            bit_depth_luma_minus8: Uint::new(0),
            bit_depth_chroma_minus8: Uint::new(0),
            avg_frame_rate: 0,
            constant_frame_rate: Uint::new(0),
            num_temporal_layers: Uint::new(1),
            temporal_id_nested: Uint::new(0),
            length_size_minus_one: Uint::new(3),
            nalu_arrays,
        },
        unknown_boxes: vec![],
    })
}

fn visual_sample_entry_fields(width: u16, height: u16) -> VisualSampleEntryFields {
    VisualSampleEntryFields {
        data_reference_index: VisualSampleEntryFields::DEFAULT_DATA_REFERENCE_INDEX,
        width,
        height,
        horizresolution: VisualSampleEntryFields::DEFAULT_HORIZRESOLUTION,
        vertresolution: VisualSampleEntryFields::DEFAULT_VERTRESOLUTION,
        frame_count: VisualSampleEntryFields::DEFAULT_FRAME_COUNT,
        compressorname: VisualSampleEntryFields::NULL_COMPRESSORNAME,
        depth: VisualSampleEntryFields::DEFAULT_DEPTH,
    }
}

/// コマンドライン引数からオプション値を取得する
fn get_arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|pos| args.get(pos + 1).cloned())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let codec = match get_arg(&args, "--codec").as_deref() {
        Some("h265" | "hevc") => Codec::H265,
        Some("h264" | "avc") | None => Codec::H264,
        Some(other) => {
            eprintln!("Unknown codec: {other} (use h264 or h265)");
            std::process::exit(1);
        }
    };
    let width = get_arg(&args, "--width")
        .map(|v| v.parse().expect("invalid --width"))
        .unwrap_or(DEFAULT_WIDTH);
    let height = get_arg(&args, "--height")
        .map(|v| v.parse().expect("invalid --height"))
        .unwrap_or(DEFAULT_HEIGHT);
    let fps = get_arg(&args, "--fps")
        .map(|v| v.parse().expect("invalid --fps"))
        .unwrap_or(DEFAULT_FPS);
    let duration_secs: f64 = get_arg(&args, "--duration")
        .map(|v| v.parse().expect("invalid --duration"))
        .unwrap_or(DEFAULT_DURATION_SECS);
    let output_path = get_arg(&args, "--output").unwrap_or_else(|| "output.mp4".to_string());

    let total_frames = (fps as f64 * duration_secs) as u64;
    let codec_name = match codec {
        Codec::H264 => "H.264",
        Codec::H265 => "H.265",
    };

    println!(
        "Encoding {codec_name} {}x{} {}fps {:.1}s ({} frames) -> {}",
        width, height, fps, duration_secs, total_frames, output_path
    );

    // raden の初期化
    let mut image = Image::new(width, height, PixelFormat::Prgb32);
    let mut runtime = PipelineRuntime::new();

    // エンコーダーの初期化
    let codec_config = match codec {
        Codec::H264 => CodecConfig::H264(H264EncoderConfig {
            profile: H264Profile::Main,
            entropy_mode: H264EntropyMode::Cabac,
        }),
        Codec::H265 => CodecConfig::Hevc(HevcEncoderConfig {
            profile: HevcProfile::Main,
            allow_open_gop: true,
        }),
    };
    let config = EncoderConfig {
        width,
        height,
        codec: codec_config,
        pixel_format: VideoPixelFormat::I420,
        average_bitrate: Some(2_000_000),
        fps_numerator: fps,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: false,
        real_time: false,
        maximize_power_efficiency: false,
        allow_frame_reordering: false,
        allow_temporal_compression: true,
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
    };
    let mut encoder = Encoder::new(config)?;

    // MP4 マルチプレクサーの初期化
    let mut muxer = Mp4FileMuxer::new()?;
    let initial_bytes = muxer.initial_boxes_bytes();
    let mut file = File::create(&output_path)?;
    file.write_all(initial_bytes)?;
    let mut data_offset = initial_bytes.len() as u64;

    let w = width as f64;
    let h = height as f64;
    let dt = 1.0 / fps as f64;
    let timescale = NonZeroU32::new(fps).unwrap();
    let mut first_keyframe = true;

    // エンコード済みフレームを MP4 に書き込む共通処理
    let write_encoded_frame = |encoded: &EncodedFrame,
                               file: &mut File,
                               muxer: &mut Mp4FileMuxer,
                               data_offset: &mut u64,
                               first_keyframe: &mut bool|
     -> Result<(), Box<dyn std::error::Error>> {
        let sample_entry = if *first_keyframe && encoded.keyframe {
            *first_keyframe = false;
            let entry = match codec {
                Codec::H264 => build_h264_sample_entry(encoded, width as u16, height as u16),
                Codec::H265 => build_h265_sample_entry(encoded, width as u16, height as u16),
            };
            Some(entry)
        } else {
            None
        };

        file.write_all(&encoded.data)?;
        let sample = Sample {
            track_kind: TrackKind::Video,
            sample_entry,
            keyframe: encoded.keyframe,
            timescale,
            duration: 1,
            data_offset: *data_offset,
            data_size: encoded.data.len(),
            composition_time_offset: None,
        };
        muxer.append_sample(&sample)?;
        *data_offset += encoded.data.len() as u64;
        Ok(())
    };

    for frame_idx in 0..total_frames {
        let t = frame_idx as f64 * dt;

        // raden で描画
        {
            let mut ctx = Context::new(&mut image, &mut runtime);
            render_frame(&mut ctx, t, w, h);
            ctx.end();
        }

        // Prgb32 -> I420 変換
        let (y, u, v) = prgb32_to_i420(
            image.data(),
            width as usize,
            height as usize,
            image.stride(),
        );

        // エンコード
        encoder.encode(
            &FrameData::I420 {
                y: &y,
                u: &u,
                v: &v,
            },
            &EncodeOptions::default(),
        )?;

        // エンコード済みフレームを MP4 に書き込む
        while let Some(encoded) = encoder.next_frame() {
            write_encoded_frame(
                &encoded,
                &mut file,
                &mut muxer,
                &mut data_offset,
                &mut first_keyframe,
            )?;
        }

        if (frame_idx + 1) % (fps as u64) == 0 {
            println!(
                "  {}/{} frames ({:.0}%)",
                frame_idx + 1,
                total_frames,
                (frame_idx + 1) as f64 / total_frames as f64 * 100.0
            );
        }
    }

    // 残りのフレームをフラッシュ
    encoder.finish()?;
    while let Some(encoded) = encoder.next_frame() {
        write_encoded_frame(
            &encoded,
            &mut file,
            &mut muxer,
            &mut data_offset,
            &mut first_keyframe,
        )?;
    }

    // MP4 ファイナライズ
    let finalized = muxer.finalize()?;
    for (offset, bytes) in finalized.offset_and_bytes_pairs() {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)?;
    }

    println!("Done: {}", output_path);

    Ok(())
}
