//! `src/decoder.rs` に対応する単体テスト

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
    DecodeHandler, DecodedFrame, Decoder, DecoderCodec, DecoderConfig, EncodedFrame, Error,
    FnDecodeHandler, FrameData, PixelFormat, VideoCodecType, supported_codecs,
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

/// H.265 デコードテスト用の VPS パラメータセット
const H265_VPS: &[u8] = &[
    64, 1, 12, 1, 255, 255, 1, 96, 0, 0, 3, 0, 144, 0, 0, 3, 0, 0, 3, 0, 90, 149, 152, 9,
];

/// H.265 デコードテスト用の SPS パラメータセット
const H265_SPS: &[u8] = &[
    66, 1, 1, 1, 96, 0, 0, 3, 0, 144, 0, 0, 3, 0, 0, 3, 0, 90, 160, 5, 2, 1, 225, 101, 149, 154,
    73, 50, 188, 5, 160, 32, 0, 0, 3, 0, 32, 0, 0, 3, 3, 33,
];

/// H.265 デコードテスト用の PPS パラメータセット
const H265_PPS: &[u8] = &[68, 1, 193, 114, 180, 98, 64];

/// H.265 デコードテスト用の NAL ユニット (I フレーム 1 枚分)
const H265_NAL_UNIT: &[u8] = &[
    40, 1, 175, 29, 16, 90, 181, 140, 90, 213, 247, 1, 91, 255, 242, 78, 254, 199, 0, 31, 209, 50,
    148, 21, 162, 38, 146, 0, 0, 3, 1, 203, 169, 113, 202, 5, 24, 129, 39, 128, 0, 0, 3, 0, 7, 204,
    147, 13, 148, 32, 0, 0, 3, 0, 0, 3, 0, 12, 24, 135, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 28, 240, 0,
    0, 3, 0, 0, 3, 0, 0, 3, 0, 8, 104, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 104, 192, 0, 0, 3, 0, 0, 3, 0,
    0, 3, 1, 223, 0, 0, 3, 0, 9, 248,
];

enum DecodeEvent {
    I420 {
        user_data: u64,
        width: usize,
        height: usize,
        y_plane: Vec<u8>,
        y_stride: usize,
        u_plane: Vec<u8>,
        u_stride: usize,
        v_plane: Vec<u8>,
        v_stride: usize,
    },
    Nv12 {
        user_data: u64,
        width: usize,
        height: usize,
        y_plane: Vec<u8>,
        y_stride: usize,
        uv_plane: Vec<u8>,
        uv_stride: usize,
    },
    Err(Error),
}

type SharedDecodeResults = Arc<Mutex<Vec<DecodeEvent>>>;

/// テスト用にビットストリームを生成する際のエンコード結果バッファ
type SharedEncodeResults<T> = Arc<Mutex<Vec<Result<EncodedFrame<T>, Error>>>>;

/// H.264 デコードテスト用のコーデック設定
fn h264_codec() -> DecoderCodec<'static> {
    DecoderCodec::H264 {
        sps: H264_SPS,
        pps: H264_PPS,
        nalu_len_bytes: 4,
    }
}

/// H.265 デコードテスト用のコーデック設定
fn h265_codec() -> DecoderCodec<'static> {
    DecoderCodec::Hevc {
        vps: H265_VPS,
        sps: H265_SPS,
        pps: H265_PPS,
        nalu_len_bytes: 4,
    }
}

/// 指定したバイト数の長さプレフィクス付き NAL ユニットのデータを構築する
///
/// `nalu_len_bytes` は Apple のドキュメントで 1, 2, 4 のいずれかが有効とされているため、
/// それぞれのケースを検証できるようにプレフィクスのバイト数を引数で受け取る。
fn nalu_data_with_prefix_len(nal_unit: &[u8], nalu_len_bytes: u32) -> Vec<u8> {
    let mut data = Vec::new();
    match nalu_len_bytes {
        1 => data.push(nal_unit.len() as u8),
        2 => data.extend_from_slice(&(nal_unit.len() as u16).to_be_bytes()),
        4 => data.extend_from_slice(&(nal_unit.len() as u32).to_be_bytes()),
        other => panic!("テストで使う長さプレフィクスとして不正な値: {other}"),
    }
    data.extend_from_slice(nal_unit);
    data
}

/// 長さプレフィクス (4 バイト) 付き NAL ユニットのデータを構築する
fn nalu_data(nal_unit: &[u8]) -> Vec<u8> {
    nalu_data_with_prefix_len(nal_unit, 4)
}

/// H.264 の長さプレフィクス (4 バイト) 付き NAL ユニットのデータを構築する
fn h264_nalu_data() -> Vec<u8> {
    nalu_data(H264_NAL_UNIT)
}

/// デコード結果を蓄積するバッファと、そのバッファに結果を積むハンドラーを返す
fn decode_collector() -> (SharedDecodeResults, FnDecodeHandler<u64>) {
    let results: SharedDecodeResults = Arc::new(Mutex::new(Vec::new()));
    let handler = FnDecodeHandler::new({
        let results = Arc::clone(&results);
        move |result: Result<DecodedFrame<u64>, Error>| {
            push_decode_event(&results, result);
        }
    });
    (results, handler)
}

/// I420 のデコード結果から取り出した各プレーン
///
/// `Nv12Frame` と違い I420 はプレーンごとにストライドが異なるため、それぞれを組で持つ。
struct I420Planes {
    /// 入力時に指定したユーザーデータ
    user_data: u64,
    /// フレームの幅
    width: usize,
    /// フレームの高さ
    height: usize,
    /// Y プレーンとそのストライド
    y: (Vec<u8>, usize),
    /// U プレーンとそのストライド
    u: (Vec<u8>, usize),
    /// V プレーンとそのストライド
    v: (Vec<u8>, usize),
}

/// `DecodedFrame::I420` から全プレーンを取り出す。他のバリアントは panic にする
fn expect_i420_planes(event: DecodeEvent) -> I420Planes {
    match event {
        DecodeEvent::I420 {
            user_data,
            width,
            height,
            y_plane,
            y_stride,
            u_plane,
            u_stride,
            v_plane,
            v_stride,
        } => I420Planes {
            user_data,
            width,
            height,
            y: (y_plane, y_stride),
            u: (u_plane, u_stride),
            v: (v_plane, v_stride),
        },
        DecodeEvent::Nv12 { .. } => unreachable!("I420 を期待したが NV12 が届いた"),
        DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
    }
}

/// デコードコールバックが `expected` 件届くまでポーリングで待つ
///
/// `Decoder::finish` は非同期デコードの完了を待つが、コールバックの登録 (ハンドラー内の
/// バッファへの push) までを保証するものではないため、テスト側で件数を確認する。
fn wait_decode_callbacks(results: &SharedDecodeResults, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let actual = results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .len();
        if actual >= expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "デコードコールバックが {expected} 件届くのを待ってタイムアウトした (現在 {actual} 件)"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// 1 件だけ届いたデコードコールバックを取り出す
fn take_single_result(results: &SharedDecodeResults) -> DecodeEvent {
    wait_decode_callbacks(results, 1);
    let callbacks = take_results(results);
    assert_eq!(callbacks.len(), 1, "デコードコールバックは 1 件届くこと");
    callbacks
        .into_iter()
        .next()
        .expect("コールバック結果が届いていない")
}

/// `DecodedFrame::I420` から user_data と幅・高さを取り出し、他のバリアントは panic にする
fn expect_i420(event: DecodeEvent) -> (u64, usize, usize) {
    match event {
        DecodeEvent::I420 {
            user_data,
            width,
            height,
            ..
        } => (user_data, width, height),
        DecodeEvent::Nv12 { .. } => unreachable!("I420 を期待したが NV12 が届いた"),
        DecodeEvent::Err(e) => panic!("想定外のデコードコールバックエラー: {e}"),
    }
}

fn push_decode_event(results: &SharedDecodeResults, result: Result<DecodedFrame<u64>, Error>) {
    let event = match result {
        Ok(DecodedFrame::I420 { frame, user_data }) => DecodeEvent::I420 {
            user_data,
            width: frame.width(),
            height: frame.height(),
            y_plane: frame.y_plane().to_vec(),
            y_stride: frame.y_stride(),
            u_plane: frame.u_plane().to_vec(),
            u_stride: frame.u_stride(),
            v_plane: frame.v_plane().to_vec(),
            v_stride: frame.v_stride(),
        },
        Ok(DecodedFrame::Nv12 { frame, user_data }) => DecodeEvent::Nv12 {
            user_data,
            width: frame.width(),
            height: frame.height(),
            y_plane: frame.y_plane().to_vec(),
            y_stride: frame.y_stride(),
            uv_plane: frame.uv_plane().to_vec(),
            uv_stride: frame.uv_stride(),
        },
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
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    let data = h264_nalu_data();
    decoder.decode(&data, 7)?;
    decoder.finish()?;

    let (user_data, width, height) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 7);
    assert_eq!(width, WIDTH as usize);
    assert_eq!(height, HEIGHT as usize);

    Ok(())
}

/// ハードコードされた H.265 ビットストリーム (VPS / SPS / PPS / IDR フレーム) をデコードし、
/// 1 フレームが出力されることを検証する。ビットストリームは 640x480 の I420 フレーム 1 枚分で、
/// ファイル内定数の H265_VPS / H265_SPS / H265_PPS / H265_NAL_UNIT を使用する (nalu_len_bytes: 4)
#[test]
fn h265_decoder() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h265_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    let data = nalu_data(H265_NAL_UNIT);
    decoder.decode(&data, 11)?;
    decoder.finish()?;

    let (user_data, width, height) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 11);
    assert_eq!(width, WIDTH as usize);
    assert_eq!(height, HEIGHT as usize);

    Ok(())
}

/// 統計値がデコードの進行に応じて増え、完了後に `in_flight_frames` が 0 に戻ることを検証する
///
/// `Decoder::stats` の参照はデコーダーと共有されているため、構築直後・送信後・
/// 出力コールバック到着後の各時点で同じ参照を読んで値を確認する。
#[test]
fn decoder_stats_counts_decoded_frames_and_outputs() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // 構築直後はセッション作成の 1 回だけが計上され、それ以外は 0 である
    let stats = decoder.stats();
    assert_eq!(stats.total_create_session_count.get(), 1);
    assert_eq!(stats.total_decode_count.get(), 0);
    assert_eq!(stats.total_output_frame_count.get(), 0);
    assert_eq!(stats.total_error_count.get(), 0);
    assert_eq!(stats.total_update_format_count.get(), 0);
    assert_eq!(stats.total_recreate_session_count.get(), 0);
    assert_eq!(stats.in_flight_frames.get(), 0);

    // 2 フレームを送信し、すべての出力が届くまで待つ
    let data = h264_nalu_data();
    for user_data in 0..2u64 {
        decoder.decode(&data, user_data)?;
    }
    decoder.finish()?;
    wait_decode_callbacks(&results, 2);

    // 統計値の計上はハンドラーの実行前に行われるため、結果の到着後は計上済みである
    let stats = decoder.stats();
    assert_eq!(
        stats.total_decode_count.get(),
        2,
        "送信したフレーム数だけ total_decode_count が増えること"
    );
    assert_eq!(
        stats.total_output_frame_count.get(),
        2,
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

/// `in_flight_frames` が送信で計上され、出力コールバックの到着で 0 に戻ることを検証する
///
/// `kVTDecodeFrame_EnableAsynchronousDecompression` を指定しているため、Video Toolbox は
/// `VTDecompressionSessionDecodeFrame` が戻る前に出力コールバックを呼ぶことがある。
/// そのため「送信が戻った直後の in-flight が 1 である」ことは保証されず、コールバックが
/// 先に届いていれば出力として計上済みである。どちらの順序でも成立する不変条件
/// (送信数 = in-flight + 出力 + エラー) で送信時の計上を検証する。
///
/// 2 フレーム分の入力として、参照関係のあるキーフレームと P フレームを使う。
#[test]
fn decoder_stats_in_flight_frames_tracks_submitted_frames() -> Result<(), Error> {
    let encoded = encode_frame_pair_with_reference();
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: &encoded.sps,
                pps: &encoded.pps,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // 送信したフレームは、コールバック未到着なら in-flight、到着済みなら出力または
    // エラーとして、必ずどちらか一方に計上される。コールバックが未到着の間にこの不変条件が
    // 崩れる場合は送信時の計上が欠けている。
    decoder.decode(&encoded.key_frame, 0)?;
    let stats = decoder.stats();
    assert_eq!(
        stats.in_flight_frames.get()
            + stats.total_output_frame_count.get()
            + stats.total_error_count.get(),
        stats.total_decode_count.get(),
        "送信したフレームが in-flight と出力のどちらにも計上されていない"
    );

    // 出力コールバックが届くと in-flight から外れる
    wait_decode_callbacks(&results, 1);
    assert_eq!(
        decoder.stats().in_flight_frames.get(),
        0,
        "出力コールバックが届いたフレームは in-flight から外れること"
    );

    // 2 件目も同じく送信で計上され、finish() の後に 0 に戻る
    decoder.decode(&encoded.p_frame, 1)?;
    let stats = decoder.stats();
    assert_eq!(
        stats.in_flight_frames.get()
            + stats.total_output_frame_count.get()
            + stats.total_error_count.get(),
        stats.total_decode_count.get(),
        "2 件目のフレームが in-flight と出力のどちらにも計上されていない"
    );
    decoder.finish()?;
    wait_decode_callbacks(&results, 2);

    let stats = decoder.stats();
    assert_eq!(stats.total_decode_count.get(), 2);
    assert_eq!(stats.total_output_frame_count.get(), 2);
    assert_eq!(stats.total_error_count.get(), 0);
    assert_eq!(
        stats.in_flight_frames.get(),
        0,
        "全フレームの出力後に in_flight_frames が 0 に戻ること"
    );

    Ok(())
}

/// 壊れたビットストリームのデコードで `total_error_count` が増え、
/// `total_output_frame_count` が増えないことを検証する
///
/// 長さプレフィクスだけを正しくした 4 バイトの中身が不正な NAL を渡す。Video Toolbox は
/// デコード失敗を出力コールバックの status で通知するため、この経路がエラー計上を通る。
#[test]
fn decoder_stats_counts_decode_errors() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // 長さプレフィクス (4 バイト) は正しいが中身が不正な NAL をデコードする
    let garbage: Vec<u8> = vec![0, 0, 0, 4, 0xFF, 0xFF, 0xFF, 0xFF];
    decoder.decode(&garbage, 0)?;
    decoder.finish()?;
    wait_decode_callbacks(&results, 1);

    // コールバックはエラーとして届く
    match take_results(&results).remove(0) {
        DecodeEvent::Err(_) => {}
        DecodeEvent::I420 { .. } => panic!("不正な NAL のデコード結果が I420 になった"),
        DecodeEvent::Nv12 { .. } => panic!("不正な NAL のデコード結果が NV12 になった"),
    }

    let stats = decoder.stats();
    assert_eq!(
        stats.total_decode_count.get(),
        1,
        "処理系に到達したフレームは total_decode_count に計上されること"
    );
    assert_eq!(
        stats.total_output_frame_count.get(),
        0,
        "デコードに失敗したフレームは出力フレーム数に計上しないこと"
    );
    assert_eq!(
        stats.total_error_count.get(),
        1,
        "デコードに失敗したフレームはエラーとして計上すること"
    );
    assert_eq!(
        stats.in_flight_frames.get(),
        0,
        "エラーで完了したフレームも in-flight から外れること"
    );

    Ok(())
}

/// H.264 デコーダーが長さプレフィクス 1 / 2 バイトの NAL ユニットをデコードできることを検証する
///
/// `nalu_len_bytes` は Apple のドキュメントで 1, 2, 4 のいずれかが有効とされているが、
/// 既存テストは 4 バイト固定のため、1 / 2 バイトの受理分岐が実行されない。
#[test]
fn h264_decoder_accepts_one_and_two_byte_nalu_length() -> Result<(), Error> {
    for nalu_len_bytes in [1u32, 2] {
        let (results, handler) = decode_collector();
        let mut decoder = Decoder::new(
            DecoderConfig {
                codec: DecoderCodec::H264 {
                    sps: H264_SPS,
                    pps: H264_PPS,
                    nalu_len_bytes,
                },
                pixel_format: PixelFormat::I420,
            },
            handler,
        )?;

        let data = nalu_data_with_prefix_len(H264_NAL_UNIT, nalu_len_bytes);
        decoder.decode(&data, nalu_len_bytes as u64)?;
        decoder.finish()?;

        let (user_data, width, height) = expect_i420(take_single_result(&results));
        assert_eq!(
            user_data, nalu_len_bytes as u64,
            "nalu_len_bytes={nalu_len_bytes} のデコード結果が届いていない"
        );
        assert_eq!(width, WIDTH as usize);
        assert_eq!(height, HEIGHT as usize);
    }
    Ok(())
}

/// H.265 デコーダーが長さプレフィクス 1 / 2 バイトの NAL ユニットをデコードできることを検証する
#[test]
fn h265_decoder_accepts_one_and_two_byte_nalu_length() -> Result<(), Error> {
    for nalu_len_bytes in [1u32, 2] {
        let (results, handler) = decode_collector();
        let mut decoder = Decoder::new(
            DecoderConfig {
                codec: DecoderCodec::Hevc {
                    vps: H265_VPS,
                    sps: H265_SPS,
                    pps: H265_PPS,
                    nalu_len_bytes,
                },
                pixel_format: PixelFormat::I420,
            },
            handler,
        )?;

        let data = nalu_data_with_prefix_len(H265_NAL_UNIT, nalu_len_bytes);
        decoder.decode(&data, nalu_len_bytes as u64)?;
        decoder.finish()?;

        let (user_data, width, height) = expect_i420(take_single_result(&results));
        assert_eq!(
            user_data, nalu_len_bytes as u64,
            "nalu_len_bytes={nalu_len_bytes} のデコード結果が届いていない"
        );
        assert_eq!(width, WIDTH as usize);
        assert_eq!(height, HEIGHT as usize);
    }
    Ok(())
}

/// ユーザー定義の [`DecodeHandler`] 実装が、独自の `UserData` / `Error` 型で動作することを検証する
///
/// 本クレートはバックエンド間でコールバックモデルを揃えるため `DecodeHandler` トレイトを
/// 公開しているが、既存テストは `FnDecodeHandler` しか使っていない。トレイトの契約
/// (`type UserData` / `type Error: From<Error>` / `on_decoded`) を独自実装で固定する。
/// なおデコードコールバックが `Err` になる経路 (壊れたビットストリーム等) は
/// Video Toolbox の挙動に依存して確実に再現できないため、ここでは正常系のみを検証する。
#[test]
fn custom_decode_handler_receives_user_data() -> Result<(), Error> {
    /// テスト内で使う結果バッファの型 (型の入れ子が深いため別名を付ける)
    type Results = Arc<Mutex<Vec<Result<(u64, String), CustomError>>>>;

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
    /// ベクターに積む。検証は `decode` / `finish` の完了後に同じテストスレッドで行う。
    /// フレームのスライスはコールバックを抜けると無効になるため、値として取り出しておく。
    struct CustomHandler {
        /// フレームの幅と user_data を蓄積する
        results: Results,
    }

    impl DecodeHandler for CustomHandler {
        type UserData = String;
        type Error = CustomError;

        fn on_decoded(&mut self, result: Result<DecodedFrame<String>, Self::Error>) {
            let entry = match result {
                Ok(DecodedFrame::I420 { frame, user_data }) => {
                    Ok((frame.width() as u64, user_data))
                }
                Ok(DecodedFrame::Nv12 { frame, user_data }) => {
                    Ok((frame.width() as u64, user_data))
                }
                Err(e) => Err(e),
            };
            self.results
                .lock()
                .expect("結果バッファの mutex が poison になっている")
                .push(entry);
        }
    }

    let results: Results = Arc::new(Mutex::new(Vec::new()));
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        CustomHandler {
            results: Arc::clone(&results),
        },
    )?;

    let data = h264_nalu_data();
    decoder.decode(&data, "custom user data".to_string())?;
    decoder.finish()?;

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
        Ok((width, user_data)) => {
            assert_eq!(width, WIDTH as u64);
            assert_eq!(user_data, "custom user data");
        }
        Err(e) => panic!("独自ハンドラーにエラーが届いた: {e:?}"),
    }
    Ok(())
}

/// H.264 と同じ SPS / PPS で `update_format` を呼ぶと、セッションを作り直さずに
/// format description だけを差し替えるパスを通ることを検証する
///
/// `VTDecompressionSessionCanAcceptFormatDescription` の戻り値は公開 API からは見えないため、
/// 「更新前のフレームを参照する P フレーム」を更新後にデコードして差し替えパスを観測する。
/// セッションを作り直すパスを通った場合は参照フレームが失われ、P フレームのデコードが
/// 参照フレーム欠落で失敗する。差し替えパスでは参照フレームが保持されるため成功する。
/// 更新後に同じキーフレームをもう一度デコードするだけでは、セッションを作り直しても成功するため
/// どちらのパスを通ったかを区別できない。
///
/// 同一の format description は `VTDecompressionSessionCanAcceptFormatDescription` が
/// 受理する前提とする。
#[test]
fn update_format_with_same_parameter_sets_keeps_reference_frames() -> Result<(), Error> {
    let encoded = encode_frame_pair_with_reference();
    let codec = || DecoderCodec::H264 {
        sps: &encoded.sps,
        pps: &encoded.pps,
        nalu_len_bytes: 4,
    };

    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // 構築時は total_create_session_count だけが 1 になる
    assert_eq!(
        decoder.stats().total_create_session_count.get(),
        1,
        "Decoder::new のセッション作成が 1 回計上されること"
    );

    // 更新前にキーフレームをデコードして参照フレームを作る
    decoder.decode(&encoded.key_frame, 1)?;
    decoder.finish()?;
    let (user_data, width, height) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 1);
    assert_eq!(width, 320);
    assert_eq!(height, 240);

    // 同一コーデック・同一パラメータセットでの更新。description 差し替えパスを通る
    decoder.update_format(codec())?;

    // 流用パスを通ったことを統計値でも確認する (セッションは再作成されない)
    assert_eq!(
        decoder.stats().total_update_format_count.get(),
        1,
        "セッションを流用した update_format が 1 回計上されること"
    );
    assert_eq!(
        decoder.stats().total_recreate_session_count.get(),
        0,
        "セッションを流用した場合は再作成が計上されないこと"
    );
    assert_eq!(
        decoder.stats().total_create_session_count.get(),
        1,
        "セッションを流用した場合は作成回数が増えないこと"
    );

    // 更新後に、更新前のキーフレームを参照する P フレームをデコードする。
    // セッションが作り直されていれば参照フレームが無いためここで失敗する。
    decoder.decode(&encoded.p_frame, 2)?;
    decoder.finish()?;
    match take_single_result(&results) {
        DecodeEvent::I420 {
            user_data,
            width,
            height,
            ..
        } => {
            assert_eq!(user_data, 2);
            assert_eq!(width, 320);
            assert_eq!(height, 240);
        }
        DecodeEvent::Err(e) => panic!(
            "description 差し替えパスで参照フレームが失われている (セッションが作り直された可能性がある): {e}"
        ),
        DecodeEvent::Nv12 { .. } => panic!("I420 を期待したが NV12 が届いた"),
    }

    Ok(())
}

/// H.264 から H.265 へ `update_format` で切り替えるとセッションが再作成され、
/// 更新後のコーデックのビットストリームをデコードできることを検証する
///
/// 同一コーデックの解像度変更は `VTDecompressionSessionCanAcceptFormatDescription` が
/// 受理し得るため、セッション再作成パスを確実に発火させるにはコーデック変更を使う。
/// 再作成されずに H.264 セッションのまま残った場合は H.265 のデコードが失敗するため、
/// 更新後のデコード成功がセッション再作成の観測になる。
#[test]
fn update_format_switching_to_h265_recreates_session() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;
    assert_eq!(
        decoder.stats().total_create_session_count.get(),
        1,
        "Decoder::new のセッション作成が 1 回計上されること"
    );

    let h264_data = h264_nalu_data();
    decoder.decode(&h264_data, 1)?;
    decoder.finish()?;
    let (user_data, ..) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 1);

    decoder.update_format(h265_codec())?;

    // 再作成パスを通ったことを統計値で確認する。create は初回と再作成の 2 回になる
    assert_eq!(
        decoder.stats().total_recreate_session_count.get(),
        1,
        "セッションを再作成した update_format が 1 回計上されること"
    );
    assert_eq!(
        decoder.stats().total_update_format_count.get(),
        0,
        "セッションを再作成した場合は流用が計上されないこと"
    );
    assert_eq!(
        decoder.stats().total_create_session_count.get(),
        2,
        "再作成ぶんの VTDecompressionSessionCreate が計上されること"
    );

    let h265_data = nalu_data(H265_NAL_UNIT);
    decoder.decode(&h265_data, 2)?;
    decoder.finish()?;
    let (user_data, width, height) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 2);
    assert_eq!(width, WIDTH as usize);
    assert_eq!(height, HEIGHT as usize);

    Ok(())
}

/// `update_format` に空のパラメータセットを渡すと InvalidConfig で拒否され、
/// デコーダーが元の状態のままデコードを続けられることを検証する
#[test]
fn update_format_rejects_empty_parameter_sets_and_keeps_session() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // 空スライスの as_ptr() は dangling pointer になるため、Video Toolbox に渡す前に拒否される
    let err = decoder
        .update_format(DecoderCodec::H264 {
            sps: &[],
            pps: H264_PPS,
            nalu_len_bytes: 4,
        })
        .expect_err("空の SPS は拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, .. } if field == "parameter_sets"
    ));

    // 拒否後もデコーダーは元のセッションでデコードできる
    let data = h264_nalu_data();
    decoder.decode(&data, 1)?;
    decoder.finish()?;
    let (user_data, ..) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 1);

    Ok(())
}

/// `update_format` に Video Toolbox が受け付けない NAL ユニット長を渡すと
/// InvalidConfig で拒否されることを検証する
#[test]
fn update_format_rejects_invalid_nalu_len_bytes() -> Result<(), Error> {
    let (_, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;

    // Apple のドキュメントで有効値は 1, 2, 4 のみ
    let err = decoder
        .update_format(DecoderCodec::H264 {
            sps: H264_SPS,
            pps: H264_PPS,
            nalu_len_bytes: 3,
        })
        .expect_err("nalu_len_bytes 3 は拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "nalu_len_bytes" && reason == "must be 1, 2, or 4"
    ));

    // HEVC 側も同じ検証を通る
    let err = decoder
        .update_format(DecoderCodec::Hevc {
            vps: H265_VPS,
            sps: H265_SPS,
            pps: H265_PPS,
            nalu_len_bytes: 0,
        })
        .expect_err("nalu_len_bytes 0 は拒否されること");
    assert!(matches!(
        err,
        Error::InvalidConfig { field, reason }
            if field == "nalu_len_bytes" && reason == "must be 1, 2, or 4"
    ));

    Ok(())
}

/// H.264 デコーダーの構築時に空のパラメータセットを渡すと InvalidConfig で拒否されることを検証する
#[test]
fn decoder_rejects_empty_h264_pps() {
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: H264_SPS,
                pps: &[],
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field, .. }) if field == "parameter_sets"
    ));
}

/// H.265 デコーダーの構築時に空のパラメータセットを渡すと InvalidConfig で拒否されることを検証する
#[test]
fn decoder_rejects_empty_h265_sps() {
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::Hevc {
                vps: H265_VPS,
                sps: &[],
                pps: H265_PPS,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field, .. }) if field == "parameter_sets"
    ));
}

/// H.264 デコーダーの構築時に Video Toolbox が受け付けない NAL ユニット長を渡すと
/// InvalidConfig で拒否されることを検証する
#[test]
fn decoder_rejects_invalid_h264_nalu_len_bytes() {
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: H264_SPS,
                pps: H264_PPS,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(r.is_ok(), "有効な設定では構築できること");

    // 有効値は 1, 2, 4 のみ。3 は拒否される
    let r = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: H264_SPS,
                pps: H264_PPS,
                nalu_len_bytes: 3,
            },
            pixel_format: PixelFormat::I420,
        },
        FnDecodeHandler::new(|_: Result<DecodedFrame<()>, Error>| {}),
    );
    assert!(matches!(
        r,
        Err(Error::InvalidConfig { field, reason })
            if field == "nalu_len_bytes" && reason == "must be 1, 2, or 4"
    ));
}

/// `PixelFormat::Nv12` を指定した H.264 デコーダーの出力が `DecodedFrame::Nv12` になり、
/// `Nv12Frame` の全メソッドが I420 相当の妥当な値を返すことを検証する
///
/// 出力ピクセルフォーマットはビットストリームではなく `DecoderConfig.pixel_format` で
/// 決まるため、専用の Nv12 ビットストリームは不要である。
#[test]
fn h264_decoder_with_nv12_output() -> Result<(), Error> {
    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: h264_codec(),
            pixel_format: PixelFormat::Nv12,
        },
        handler,
    )?;

    let data = h264_nalu_data();
    decoder.decode(&data, 7)?;
    decoder.finish()?;

    let event = take_single_result(&results);
    let DecodeEvent::Nv12 {
        user_data,
        width,
        height,
        y_plane,
        y_stride,
        uv_plane,
        uv_stride,
    } = event
    else {
        panic!("NV12 のデコード結果が届いていない");
    };

    assert_eq!(user_data, 7);
    assert_eq!(width, WIDTH as usize);
    assert_eq!(height, HEIGHT as usize);

    // y_plane は height 行 × y_stride バイト。実際の幅は y_stride 以上ある
    assert_eq!(y_plane.len(), height * y_stride);
    assert!(y_stride >= width, "y_stride は幅以上であること");
    // uv_plane は height / 2 行 × uv_stride バイトのインターリーブ
    assert_eq!(uv_plane.len(), height.div_ceil(2) * uv_stride);
    assert!(uv_stride >= width, "uv_stride は幅以上であること");

    // Y プレーンの先頭行にパディングを除いて有効な画素が入っていること
    let first_row = &y_plane[..width];
    assert!(
        first_row.iter().any(|&v| v != 0),
        "Y プレーンの先頭行が全て 0 になっている"
    );

    Ok(())
}

/// `PixelFormat::Nv12` でデコードした結果の UV プレーンが、元の I420 の U / V と
/// 同等の内容になっていることを PSNR で検証する
///
/// NV12 の UV プレーンは U / V が 1 バイトおきに交互に並ぶ。PSNR は同一位置の
/// バイト同士を比較するだけなので、偶数番目を U、奇数番目を V とみなして比較できる。
#[test]
fn h264_decoder_nv12_uv_plane_matches_source() -> Result<(), Error> {
    let width = 320usize;
    let height = 240usize;
    let (source_y, source_u, source_v) = generate_colorbar_i420(width, height);

    // H.264 エンコーダーでカラーバーを 1 フレームだけエンコードする
    let encoded = encode_colorbar_frame(&source_y, &source_u, &source_v);

    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: &encoded.sps,
                pps: &encoded.pps,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::Nv12,
        },
        handler,
    )?;
    decoder.decode(&encoded.data, 1)?;
    decoder.finish()?;

    let DecodeEvent::Nv12 {
        y_plane,
        y_stride,
        uv_plane,
        uv_stride,
        ..
    } = take_single_result(&results)
    else {
        panic!("NV12 のデコード結果が届いていない");
    };

    let decoded_u: Vec<u8> = uv_plane.iter().step_by(2).copied().collect();
    let decoded_v: Vec<u8> = uv_plane.iter().skip(1).step_by(2).copied().collect();
    let decoded_uv_stride = uv_stride.div_ceil(2);

    let psnr_u = psnr_plane(
        &source_u,
        width / 2,
        &decoded_u,
        decoded_uv_stride,
        width / 2,
        height / 2,
    );
    let psnr_v = psnr_plane(
        &source_v,
        width / 2,
        &decoded_v,
        decoded_uv_stride,
        width / 2,
        height / 2,
    );
    let psnr_y = psnr_plane(&source_y, width, &y_plane, y_stride, width, height);
    assert!(psnr_y >= 25.0, "Y プレーンの PSNR が低い: {psnr_y:.1} dB");
    assert!(psnr_u >= 25.0, "U プレーンの PSNR が低い: {psnr_u:.1} dB");
    assert!(psnr_v >= 25.0, "V プレーンの PSNR が低い: {psnr_v:.1} dB");

    Ok(())
}

/// I420 でデコードした結果の U / V プレーンが、元のカラーバーの U / V と
/// 同等の内容になっていることを PSNR で検証する
///
/// `I420Frame` の `u_plane` / `v_plane` / `u_stride` / `v_stride` は既存テストで
/// 一度も呼ばれておらず、プレーン番号や行数の計算が誤っていても検出できなかった。
#[test]
fn h264_decoder_i420_uv_planes_match_source() -> Result<(), Error> {
    let width = 320usize;
    let height = 240usize;
    let (source_y, source_u, source_v) = generate_colorbar_i420(width, height);
    let encoded = encode_colorbar_frame(&source_y, &source_u, &source_v);

    let (results, handler) = decode_collector();
    let mut decoder = Decoder::new(
        DecoderConfig {
            codec: DecoderCodec::H264 {
                sps: &encoded.sps,
                pps: &encoded.pps,
                nalu_len_bytes: 4,
            },
            pixel_format: PixelFormat::I420,
        },
        handler,
    )?;
    decoder.decode(&encoded.data, 1)?;
    decoder.finish()?;

    let planes = expect_i420_planes(take_single_result(&results));
    assert_eq!(planes.user_data, 1);
    assert_eq!(planes.width, width, "デコード結果の幅が一致しない");
    assert_eq!(planes.height, height, "デコード結果の高さが一致しない");

    // プレーンの長さは (高さ / 2 の切り上げ) 行 × ストライドと一致すること
    let (y_plane, y_stride) = &planes.y;
    let (u_plane, u_stride) = &planes.u;
    let (v_plane, v_stride) = &planes.v;
    assert_eq!(
        y_plane.len(),
        height * y_stride,
        "Y プレーンの長さがストライドと一致しない"
    );
    assert_eq!(
        u_plane.len(),
        height.div_ceil(2) * u_stride,
        "U プレーンの長さがストライドと一致しない"
    );
    assert_eq!(
        v_plane.len(),
        height.div_ceil(2) * v_stride,
        "V プレーンの長さがストライドと一致しない"
    );
    assert!(*y_stride >= width, "y_stride は幅以上であること");
    assert!(
        *u_stride >= width.div_ceil(2),
        "u_stride は半分の幅以上であること"
    );
    assert!(
        *v_stride >= width.div_ceil(2),
        "v_stride は半分の幅以上であること"
    );

    let psnr_y = psnr_plane(&source_y, width, y_plane, *y_stride, width, height);
    let psnr_u = psnr_plane(
        &source_u,
        width / 2,
        u_plane,
        *u_stride,
        width / 2,
        height / 2,
    );
    let psnr_v = psnr_plane(
        &source_v,
        width / 2,
        v_plane,
        *v_stride,
        width / 2,
        height / 2,
    );
    assert!(psnr_y >= 25.0, "Y プレーンの PSNR が低い: {psnr_y:.1} dB");
    assert!(
        psnr_u >= 25.0,
        "U プレーンの PSNR が低い: {psnr_u:.1} dB (プレーンの取り違えの可能性)"
    );
    assert!(
        psnr_v >= 25.0,
        "V プレーンの PSNR が低い: {psnr_v:.1} dB (プレーンの取り違えの可能性)"
    );

    Ok(())
}

/// AV1 デコーダーが構築できることを検証する。非対応環境では supported_codecs() の
/// 事前検査でスキップする。ビットストリームなしの最小構築ではコーデック固有の
/// パラメータ不足で UnsupportedCodec が返り得るため、Ok と UnsupportedCodec の両方を許容する
#[test]
fn init_av1_decoder() -> Result<(), Error> {
    if !supported_codecs()
        .iter()
        .any(|c| c.codec == VideoCodecType::Av1 && c.decoding.hardware_accelerated)
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
fn psnr_plane(
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

/// Y プレーン同士の PSNR を計算する（dB）
fn psnr_y(
    original: &[u8],
    original_stride: usize,
    decoded: &[u8],
    decoded_stride: usize,
    width: usize,
    height: usize,
) -> f64 {
    psnr_plane(
        original,
        original_stride,
        decoded,
        decoded_stride,
        width,
        height,
    )
}

/// テスト用に 1 フレームだけエンコードした結果
struct EncodedTestFrame {
    /// SPS
    sps: Vec<u8>,
    /// PPS
    pps: Vec<u8>,
    /// AVCC 形式 (長さプレフィクス付き) の圧縮データ
    data: Vec<u8>,
}

/// テスト用ビットストリーム生成のヘルパーが返すエラー
///
/// `Decoder` のテストは `shiguredo_video_toolbox::Error` をそのまま返すが、
/// ヘルパーは「結果が届かない」等のテスト都合の失敗も扱うため別の型にする。
#[derive(Debug)]
enum HelperError {
    /// ライブラリのエラー
    Library(Error),
    /// テストの前提が満たされなかった
    Assertion(String),
}

impl From<Error> for HelperError {
    fn from(e: Error) -> Self {
        Self::Library(e)
    }
}

impl std::fmt::Display for HelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Library(e) => write!(f, "library error: {e}"),
            Self::Assertion(message) => write!(f, "assertion failed: {message}"),
        }
    }
}

/// テスト用のビットストリームを生成する
///
/// デコーダーのテストで任意の解像度・内容のビットストリームを得るために使う。
/// キーフレームとして出力するため、パラメータセットが 1 組ずつ得られる。
///
/// `pixel_format` はエンコーダーに渡す入力フォーマットで、`FrameData` のバリアントと
/// 一致している必要がある。出力フォーマット (デコード側の `PixelFormat`) とは独立である。
fn encode_test_frame(
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    frame: &FrameData<'_>,
) -> Result<EncodedTestFrame, HelperError> {
    use shiguredo_video_toolbox::{
        CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, FnEncodeHandler,
        H264EncoderConfig, H264EntropyMode, H264Profile,
    };

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut encoder = Encoder::new(
        EncoderConfig {
            width,
            height,
            codec: CodecConfig::H264(H264EncoderConfig {
                profile: H264Profile::Main,
                entropy_mode: H264EntropyMode::Cabac,
            }),
            pixel_format,
            average_bitrate: Some(500_000),
            fps_numerator: 30,
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
        },
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

    encoder.encode(
        frame,
        &EncodeOptions {
            force_key_frame: true,
        },
        0,
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
        if Instant::now() >= deadline {
            return Err(HelperError::Assertion(
                "エンコード結果が届くのを待ってタイムアウトした".to_string(),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }

    let mut guard = results
        .lock()
        .expect("結果バッファの mutex が poison になっている");
    let frame = guard
        .pop()
        .expect("エンコード結果が届いていない")
        .expect("エンコードが失敗している");
    if !frame.keyframe {
        return Err(HelperError::Assertion(
            "force_key_frame でエンコードしたフレームはキーフレームであること".to_string(),
        ));
    }
    if frame.sps_list.len() != 1 {
        return Err(HelperError::Assertion(
            "キーフレームには SPS が 1 組付くこと".to_string(),
        ));
    }
    if frame.pps_list.len() != 1 {
        return Err(HelperError::Assertion(
            "キーフレームには PPS が 1 組付くこと".to_string(),
        ));
    }

    Ok(EncodedTestFrame {
        sps: frame.sps_list[0].clone(),
        pps: frame.pps_list[0].clone(),
        data: frame.data,
    })
}

/// カラーバーの I420 フレームを H.264 で 1 枚だけエンコードし、デコーダーに渡す素材を返す
///
/// I420 / NV12 のどちらの出力フォーマットでデコードする場合も、入力は同じ I420 でよい。
fn encode_colorbar_frame(y: &[u8], u: &[u8], v: &[u8]) -> EncodedTestFrame {
    let width = 320u32;
    let height = 240u32;
    encode_test_frame(
        width,
        height,
        PixelFormat::I420,
        &FrameData::I420 { y, u, v },
    )
    .expect("カラーバーのエンコードに失敗した")
}

/// 参照関係のある 2 フレーム (キーフレームと、それを参照する P フレーム) のエンコード結果
struct EncodedFramePair {
    /// SPS
    sps: Vec<u8>,
    /// PPS
    pps: Vec<u8>,
    /// キーフレームのデータ
    key_frame: Vec<u8>,
    /// キーフレームを参照する P フレームのデータ
    p_frame: Vec<u8>,
}

/// 参照関係のある 2 フレームを H.264 でエンコードし、デコーダーに渡す素材を返す
///
/// `update_format` が description を差し替えるパスとセッションを作り直すパスを区別するために使う。
/// セッションを作り直すと参照フレーム (DPB) が失われるため、更新後に P フレームをデコードすると
/// 参照フレーム欠落で失敗する。差し替えパスでは参照フレームが保持されるため成功する。
///
/// 2 フレーム目が確実にキーフレーム以外になるよう、キーフレーム間隔を十分に長くする。
fn encode_frame_pair_with_reference() -> EncodedFramePair {
    use shiguredo_video_toolbox::{
        CodecConfig, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, FnEncodeHandler,
        H264EncoderConfig, H264EntropyMode, H264Profile,
    };

    /// キーフレームを識別するユーザーデータ
    const KEY_FRAME_USER_DATA: u64 = 0;
    /// P フレームを識別するユーザーデータ
    const P_FRAME_USER_DATA: u64 = 1;

    let width = 320u32;
    let height = 240u32;
    // P フレームが参照する対象があればよいため、2 フレームとも同じ内容でよい
    let (y, u, v) = generate_colorbar_i420(width as usize, height as usize);

    let results: SharedEncodeResults<u64> = Arc::new(Mutex::new(Vec::new()));
    let mut config = EncoderConfig {
        width,
        height,
        codec: CodecConfig::H264(H264EncoderConfig {
            profile: H264Profile::Main,
            entropy_mode: H264EntropyMode::Cabac,
        }),
        pixel_format: PixelFormat::I420,
        average_bitrate: Some(500_000),
        fps_numerator: 30,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: false,
        real_time: false,
        maximize_power_efficiency: false,
        allow_frame_reordering: false,
        // 2 フレーム目が非参照フレームにならないよう、時間方向の圧縮を無効にする
        allow_temporal_compression: false,
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
        data_rate_limits: Vec::new(),
    };
    // 自動でキーフレームが挿入されないよう、間隔を 2 フレームより十分に長くする
    config.max_key_frame_interval = std::num::NonZeroU32::new(300);

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
    )
    .expect("エンコーダーの構築に失敗した");

    let frame = FrameData::I420 {
        y: &y,
        u: &u,
        v: &v,
    };
    encoder
        .encode(
            &frame,
            &EncodeOptions {
                force_key_frame: true,
            },
            KEY_FRAME_USER_DATA,
        )
        .expect("キーフレームのエンコードに失敗した");
    encoder
        .encode(
            &frame,
            &EncodeOptions {
                force_key_frame: false,
            },
            P_FRAME_USER_DATA,
        )
        .expect("P フレームのエンコードに失敗した");
    encoder.finish().expect("エンコーダーの終了に失敗した");

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if results
            .lock()
            .expect("結果バッファの mutex が poison になっている")
            .len()
            >= 2
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "エンコード結果が 2 件届くのを待ってタイムアウトした"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let mut guard = results
        .lock()
        .expect("結果バッファの mutex が poison になっている");
    let frames = std::mem::take(&mut *guard);
    drop(guard);

    // コールバックの到着順に依存しないよう、user_data で識別する
    let mut key_frame = None;
    let mut p_frame = None;
    for frame in frames {
        let frame = frame.expect("エンコードが失敗している");
        if frame.user_data == KEY_FRAME_USER_DATA {
            key_frame = Some(frame);
        } else {
            assert_eq!(
                frame.user_data, P_FRAME_USER_DATA,
                "想定外の user_data が届いた"
            );
            p_frame = Some(frame);
        }
    }

    let key_frame = key_frame.expect("キーフレームのエンコード結果が届いていない");
    let p_frame = p_frame.expect("P フレームのエンコード結果が届いていない");
    assert!(key_frame.keyframe, "1 フレーム目はキーフレームであること");
    assert!(!p_frame.keyframe, "2 フレーム目はキーフレームでないこと");
    assert_eq!(key_frame.sps_list.len(), 1, "SPS は 1 組得られること");
    assert_eq!(key_frame.pps_list.len(), 1, "PPS は 1 組得られること");

    EncodedFramePair {
        sps: key_frame.sps_list[0].clone(),
        pps: key_frame.pps_list[0].clone(),
        key_frame: key_frame.data,
        p_frame: p_frame.data,
    }
}

/// shiguredo_libvpx の VP9 エンコーダーで生成したフレームをデコードし、
/// Y プレーンの PSNR が下限を満たすことを検証する (対応環境でない場合はスキップ)
#[test]
fn vp9_decoder() -> Result<(), Error> {
    if !supported_codecs()
        .iter()
        .any(|c| c.codec == VideoCodecType::Vp9 && c.decoding.hardware_accelerated)
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
                ..
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
            codec: h264_codec(),
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

    let (user_data, ..) = expect_i420(take_single_result(&results));
    assert_eq!(user_data, 2);
    Ok(())
}
