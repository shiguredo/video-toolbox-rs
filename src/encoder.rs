//! H.264 / H.265 エンコーダー

mod callback;
mod config;
mod frame;
mod handler;
mod pixel_buffer;
mod session;
mod stats;
mod validation;

pub use config::{
    CodecConfig, DataRateLimit, EncodeOptions, EncoderConfig, H264EncoderConfig, H264EntropyMode,
    H264Profile, HevcEncoderConfig, HevcProfile, ReconfigureParams,
};
pub use frame::{EncodedFrame, FrameData};
pub use handler::{EncodeHandler, FnEncodeHandler};
pub use stats::EncoderStats;

use std::ffi::c_void;
use std::sync::Arc;

use crate::{
    encoder::session::{push_bitrate_property, push_expected_frame_rate_property},
    error::Error,
    sys,
    types::{CfPtr, cf_dictionary},
};

/// Video Toolbox のコールバックへ渡すハンドラーと統計値の組
///
/// `VTCompressionSessionCreate` の `outputCallbackRefCon` にこの Box の中身のポインタを渡す。
/// コールバックは Video Toolbox のコールバックスレッドから呼ばれるため、統計値は `Arc` で
/// 共有し、[`Encoder`] も同じ統計値を参照できるようにする。
struct EncodeCallbackContext<H: EncodeHandler> {
    /// ユーザーが指定したハンドラー
    handler: H,
    /// コールバックと共有する統計値
    stats: Arc<EncoderStats>,
}

/// H.264 / H.265 エンコーダー
///
/// エンコード完了時に [`EncodeHandler::on_encoded`] を呼び出す。
/// この [`EncodeHandler::on_encoded`] の呼び出しは Video Toolbox のコールバックスレッドから行われる。
pub struct Encoder<H: EncodeHandler> {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    // FFI の outputCallbackRefCon にこの Box の中身ポインタを渡しているため、
    // Encoder の生存期間中は保持し続ける必要がある。
    context: Box<EncodeCallbackContext<H>>,
}

impl<H: EncodeHandler> Encoder<H> {
    /// エンコーダーのインスタンスを生成する
    pub fn new(config: EncoderConfig, handler: H) -> Result<Self, Error> {
        Self::validate_config(&config)?;
        let context = Box::new(EncodeCallbackContext {
            handler,
            stats: Arc::new(EncoderStats::default()),
        });
        let session = unsafe { Self::create_compression_session(&config, context.as_ref())? };

        Ok(Self {
            session,
            config,
            next_input_pts: 0,
            context,
        })
    }

    /// エンコーダーの統計値を返す
    ///
    /// 戻り値はエンコーダーと共有されている統計値への参照である。エンコーダーを
    /// 操作するスレッドと Video Toolbox のコールバックスレッドの両方が随時更新する
    /// ため、複数のフィールドを読む間に値が変化し得る。値を保存しておきたい場合は
    /// `clone()` する。
    pub fn stats(&self) -> &EncoderStats {
        &self.context.stats
    }

    /// 現在エンコーダーが内部で保持している設定を返す
    ///
    /// 戻り値は [`Encoder::new`] で渡した値、または直近の [`Encoder::reconfigure`] 呼び出しで
    /// 反映された値である。Video Toolbox がバックエンドで丸めた実効値とは異なる場合がある。
    ///
    /// [`Encoder::reconfigure`] 経由で動的に更新され得るのは [`ReconfigureParams`] に
    /// 対応するフィールドのみである。`average_bitrate` は同名のフィールドが、
    /// `fps_numerator` / `fps_denominator` は `expected_frame_rate` の指定によって
    /// 書き換わる。その他のフィールドは [`Encoder::new`] で渡した初期値のまま保持される。
    pub fn config(&self) -> &EncoderConfig {
        &self.config
    }

    /// 動的に変更可能なエンコードパラメータを更新する
    ///
    /// `VTSessionSetProperties` を 1 回呼び出して指定された項目を一括反映する。
    /// セッション再作成は行わないため、未出力フレームの自動フラッシュも行わない。
    /// フラッシュが必要なら呼び出し側で先に [`Encoder::finish`] を明示する。
    ///
    /// 全項目 `None` の場合は no-op として `Ok(())` を返す。
    /// `VTSessionSetProperties` が失敗した場合は `self.config` を変更せず、セッションも生かしたままエラーを返す。
    ///
    /// `expected_frame_rate` を更新した場合は `fps_numerator` / `fps_denominator` が
    /// `expected_frame_rate / 1` に正規化される (分数 fps は保持されない。分数 fps を
    /// 保持したい場合は [`Encoder`] を作り直す)。また、内部の
    /// `next_input_pts` を新しい timescale に切り上げで再スケールし、直前出力フレームと
    /// 物理時間として単調増加するようにする。切り上げのため 1 回の更新につき最大
    /// 1/`expected_frame_rate` 秒だけ PTS が前倒しされ、頻繁に更新すると累積し得る。
    /// 再スケール結果が `i64` を超えた場合は [`Error::LimitExceeded`] を返す。
    ///
    /// 解像度・コーデック・ピクセルフォーマットの変更はこの API では対応しないため [`Encoder`] を作り直す。
    pub fn reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error> {
        Self::validate_reconfigure_params(&params)?;

        // VTSessionSetProperties 実行前に `next_input_pts` の再スケール値を確定させる。
        // FFI 呼び出しが失敗した場合はここまでで巻き戻し、self の状態を変更しない。
        // 直前出力フレームとの PTS 単調性を維持するため切り上げ (div_ceil) で再スケールする。
        // 切り捨てだと new_timescale < old_timescale 時に rescaled が潰れて逆行し得る。
        // `next_input_pts` は 0 開始で加算しかされない非負値なので u128 で計算できる
        // (符号付き整数の div_ceil は unstable のため)。
        let rescaled_next_input_pts = if let Some(fps) = params.expected_frame_rate {
            let old_timescale = self.config.fps_numerator as u128;
            let new_timescale = fps as u128;
            let old_pts = self.next_input_pts as u128;
            let rescaled = (old_pts * new_timescale).div_ceil(old_timescale);
            if rescaled > i64::MAX as u128 {
                return Err(Error::LimitExceeded {
                    reason: "rescaled presentation timestamp overflow".into(),
                });
            }
            Some(rescaled as i64)
        } else {
            None
        };

        unsafe {
            let mut properties: Vec<(sys::CFStringRef, *const c_void)> = Vec::new();
            let mut cf_objects: Vec<CfPtr<c_void>> = Vec::new();

            if let Some(bitrate) = params.average_bitrate {
                push_bitrate_property(&mut properties, &mut cf_objects, bitrate)?;
            }
            if let Some(fps) = params.expected_frame_rate {
                push_expected_frame_rate_property(&mut properties, &mut cf_objects, fps)?;
            }

            // 更新対象が無ければ no-op (パラメータのフィールド列挙で判定するとフィールド追加時に漏れる)
            if properties.is_empty() {
                return Ok(());
            }

            let properties_dict = cf_dictionary(&properties)?;
            let status = sys::VTSessionSetProperties(self.session.cast(), properties_dict.0.cast());
            Error::check(status, "VTSessionSetProperties")?;

            // 更新対象が無い no-op と失敗時はここに到達しないため、成功した更新だけを計上する
            self.context.stats.total_reconfigure_count.inc();
        }

        // FFI 成功時のみ self の状態を更新する。
        if let Some(bitrate) = params.average_bitrate {
            self.config.average_bitrate = Some(bitrate);
        }
        if let Some(fps) = params.expected_frame_rate {
            // ExpectedFrameRate は単一整数のため分母を 1 に正規化する。
            self.config.fps_numerator = fps;
            self.config.fps_denominator = 1;
        }
        if let Some(new_pts) = rescaled_next_input_pts {
            self.next_input_pts = new_pts;
        }

        Ok(())
    }

    /// これ以上データが来ないことをエンコーダーに伝える
    ///
    /// 残りのエンコード結果は構築時に指定したコールバックで通知される
    pub fn finish(&mut self) -> Result<(), Error> {
        unsafe {
            let status = sys::VTCompressionSessionCompleteFrames(self.session, sys::kCMTimeInvalid);
            Error::check(status, "VTCompressionSessionCompleteFrames")?;
        }
        Ok(())
    }
}

impl<H: EncodeHandler> Drop for Encoder<H> {
    fn drop(&mut self) {
        unsafe {
            sys::VTCompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);
        }
    }
}

// SAFETY: VTCompressionSession は内部でスレッドセーフに管理されており、
// Apple のドキュメントでもセッションの操作は異なるスレッドから呼び出し可能とされている。
// ハンドラーは Box<EncodeCallbackContext<H>> でヒープに隔離されており、
// Encoder の生存期間中はアドレス不変である。
unsafe impl<H: EncodeHandler> Send for Encoder<H> {}

#[cfg(test)]
mod tests {
    //! `Encoder::reconfigure` / `Encoder::encode` / `Encoder::encode_pixel_buffer` の内部状態
    //! (`next_input_pts`) と、Video Toolbox のプロパティ反映挙動を直接確認するためのテスト。
    //! `tests/test_encoder.rs` からは到達できない private フィールドや FFI の直接呼び出しが
    //! 必要なテストだけをここに置く。通常系の検証は `tests/test_encoder.rs` 側にある。

    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{
        PixelFormat, sys,
        types::{CfPtrMut, cf_array, cf_number_f64, cf_number_i64},
    };

    /// データレート上限の検証に使う上限値 (750 kbps = 1 秒あたり 93,750 バイト)
    const LIMIT_BYTES_PER_SEC: u64 = 93_750;

    /// 出力バイト数を集計するウィンドウのフレーム数 (30 fps のとき 1 秒ぶん)
    const TEST_WINDOW_FRAMES: usize = 30;

    /// エンコードコールバックで受け取った結果を蓄積する共有バッファ
    type SharedEncodeResults<T> = Arc<Mutex<Vec<Result<EncodedFrame<T>, Error>>>>;

    /// エンコードコールバックが `expected` 件届くまでポーリングで待つ
    fn wait_for_encode_callbacks(count: &AtomicUsize, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let actual = count.load(Ordering::SeqCst);
            if actual >= expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "エンコードコールバックが {expected} 件届くのを待ってタイムアウトした (現在 {actual} 件)"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// `next_input_pts` を `i64::MAX` にした状態で、送信が PTS オーバーフローで拒否されることを検証する
    ///
    /// 検証内容: 送信 API が `Error::LimitExceeded` を返す、`next_input_pts` が不変、
    /// 以後の呼び出しも同じエラーを返す、フレームが送信されない (コールバックが増えない)。
    fn assert_pts_overflow_rejected<H: EncodeHandler>(
        encoder: &mut Encoder<H>,
        mut submit: impl FnMut(&mut Encoder<H>) -> Result<(), Error>,
        callback_count: &AtomicUsize,
        baseline: usize,
    ) {
        // private フィールドへ直接書き込んでオーバーフロー境界を作る
        // (公開 API 経由では送信を i64::MAX 回呼ぶ必要があるため)
        encoder.next_input_pts = i64::MAX;

        // オーバーフローする PTS は送信されず、エラーが返る
        let err = submit(encoder).expect_err("PTS オーバーフローは拒否されること");
        assert!(matches!(
            err,
            Error::LimitExceeded { reason } if reason == "input presentation timestamp overflow",
        ));

        // next_input_pts は変更されない
        assert_eq!(encoder.next_input_pts, i64::MAX);

        // エラー後も next_input_pts が動かないため、以後の呼び出しも同じエラーを返す
        let err =
            submit(encoder).expect_err("PTS オーバーフローは以後の呼び出しでも拒否されること");
        assert!(matches!(
            err,
            Error::LimitExceeded { reason } if reason == "input presentation timestamp overflow",
        ));

        // フレームが万一送信された場合のコールバック到達を待つため 1 秒待ってから、
        // コールバックが増えていない (フレームを送信していない) ことを確認する
        thread::sleep(Duration::from_secs(1));
        assert_eq!(
            callback_count.load(Ordering::SeqCst),
            baseline,
            "オーバーフロー時はフレームを送信しないこと"
        );
    }

    fn base_encoder_config() -> EncoderConfig {
        EncoderConfig {
            width: 960,
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

    fn black_i420_frame(w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let uv_w = w.div_ceil(2);
        let uv_h = h.div_ceil(2);
        (
            vec![0u8; w * h],
            vec![0u8; uv_w * uv_h],
            vec![0u8; uv_w * uv_h],
        )
    }

    fn noop_handler() -> FnEncodeHandler<()> {
        FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})
    }

    /// データレート上限の効果を観測するための合成フレーム (グラデーション + 下部 1/4 ノイズ) を生成する
    ///
    /// ノイズ帯がビット消費を押し上げるため、上限を設定すると出力バイト数が明確に抑えられる。
    /// 全面ノイズでは最低品質でも上限を下回れず、上限の効果を観測できない。
    fn noisy_i420_frame(
        w: usize,
        h: usize,
        frame_index: usize,
        seed: &mut u64,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut y_plane = vec![0u8; w * h];
        for row in 0..h {
            for col in 0..w {
                y_plane[row * w + col] = ((col * 255 / w + frame_index * 3) % 256) as u8;
            }
        }
        for b in y_plane[w * h * 3 / 4..].iter_mut() {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *b = (*seed >> 33) as u8;
        }
        let uv_size = w.div_ceil(2) * h.div_ceil(2);
        (y_plane, vec![128u8; uv_size], vec![128u8; uv_size])
    }

    /// `data_rate_limits` の効果を観測するシナリオを実行し、
    /// [`TEST_WINDOW_FRAMES`] フレームごとの出力バイト数を返す
    ///
    /// `set_at_construction` が true の場合は構築時に上限を設定する。false の場合は
    /// 30 フレームエンコードした後に `VTSessionSetProperty` で直接設定する。エンコード中の
    /// 上限変更は本クレートの公開 API では提供していない (Video Toolbox が受け付けない) ため、
    /// Video Toolbox の挙動そのものを確認する目的で FFI を直接呼ぶ。
    fn data_rate_limit_windowed_output(set_at_construction: bool) -> Result<Vec<u64>, Error> {
        const W: usize = 960;
        const H: usize = 480;
        const FRAMES: usize = 120;

        let mut config = base_encoder_config();
        config.average_bitrate = Some(2_000_000);
        config.fps_numerator = TEST_WINDOW_FRAMES as u32;
        config.fps_denominator = 1;
        config.real_time = true;
        config.prioritize_encoding_speed_over_quality = true;
        config.data_rate_limits = if set_at_construction {
            vec![DataRateLimit {
                bytes: LIMIT_BYTES_PER_SEC,
                window: Duration::from_secs(1),
            }]
        } else {
            Vec::new()
        };

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
            if i == TEST_WINDOW_FRAMES && !set_at_construction {
                // エンコード開始後 (30 フレーム投入後) に DataRateLimits を直接設定する。
                // kVTCompressionPropertyKey_DataRateLimits は「bytes, seconds」を交互に並べた
                // 偶数個の CFNumber の CFArray を取る (VTCompressionProperties.h)。
                let bytes = cf_number_i64(LIMIT_BYTES_PER_SEC as i64)?;
                let seconds = cf_number_f64(1.0)?;
                let array = cf_array(&[bytes.0, seconds.0])?;
                let status = unsafe {
                    sys::VTSessionSetProperty(
                        encoder.session.cast(),
                        sys::kVTCompressionPropertyKey_DataRateLimits,
                        array.0.cast(),
                    )
                };
                Error::check(status, "VTSessionSetProperty")?;
            }
            let (y, u, v) = noisy_i420_frame(W, H, i, &mut seed);
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

        // 全フレームぶんのエンコード結果が届くまで待つ
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let count = results
                .lock()
                .expect("結果バッファの mutex が poison になっている")
                .len();
            if count >= FRAMES {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "エンコード結果が {FRAMES} 件届くのを待ってタイムアウトした (現在 {count} 件)"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let mut sizes = Vec::new();
        {
            let mut guard = results
                .lock()
                .expect("結果バッファの mutex が poison になっている");
            for result in guard.drain(..) {
                match result {
                    Ok(frame) => sizes.push(frame.data.len() as u64),
                    Err(e) => panic!("想定外のエンコードコールバックエラー: {e}"),
                }
            }
        }
        let windows: Vec<u64> = sizes
            .chunks(TEST_WINDOW_FRAMES)
            .map(|chunk| chunk.iter().sum())
            .collect();
        eprintln!("データレート上限のシナリオ (構築時設定={set_at_construction}): {windows:?}");
        Ok(windows)
    }

    #[test]
    fn reconfigure_rescales_next_input_pts_on_frame_rate_change() -> Result<(), Error> {
        // 30000/1001 (29.97 fps) で開始し、60 fps に reconfigure すると
        // `next_input_pts` が新しい timescale (= 60) に再スケールされることを確認する。
        let mut config = base_encoder_config();
        config.fps_numerator = 30_000;
        config.fps_denominator = 1_001;
        let mut encoder = Encoder::new(config, noop_handler())?;

        let (y, u, v) = black_i420_frame(960, 480);
        let frame = FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        };
        // 2 フレームを encode して next_input_pts = 2 * 1001 = 2002
        encoder.encode(&frame, &EncodeOptions::default(), ())?;
        encoder.encode(&frame, &EncodeOptions::default(), ())?;
        assert_eq!(encoder.next_input_pts, 2 * 1001);

        encoder.reconfigure(ReconfigureParams {
            expected_frame_rate: Some(60),
            ..Default::default()
        })?;

        // 物理時間 2002/30000 ≒ 0.0667 秒に対応する新 timescale 60 での PTS は 4.004。
        // PTS の単調性を保つため切り上げ (div_ceil) を採用しているので 5 となる。
        assert_eq!(encoder.next_input_pts, 5);
        Ok(())
    }

    #[test]
    fn reconfigure_preserves_next_input_pts_when_only_bitrate_changes() -> Result<(), Error> {
        // expected_frame_rate を指定しない場合、next_input_pts は再スケールされない。
        let config = base_encoder_config();
        let mut encoder = Encoder::new(config, noop_handler())?;

        let (y, u, v) = black_i420_frame(960, 480);
        let frame = FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        };
        encoder.encode(&frame, &EncodeOptions::default(), ())?;
        encoder.encode(&frame, &EncodeOptions::default(), ())?;
        let before = encoder.next_input_pts;

        encoder.reconfigure(ReconfigureParams {
            average_bitrate: Some(500_000),
            ..Default::default()
        })?;

        assert_eq!(encoder.next_input_pts, before);
        Ok(())
    }

    #[test]
    fn reconfigure_overflows_when_rescaled_pts_exceeds_i64_max() -> Result<(), Error> {
        // i64 の上限を意図的に超える条件で再スケールし、`Error::LimitExceeded` が返ることを確認する。
        // 公開 API 経由で next_input_pts を i64::MAX にするには encode を i64::MAX 回呼ぶ必要があるため、
        // private フィールドへ直接書き込んで境界条件を作る (このモジュールツリーからのみ可能)。
        let config = base_encoder_config();
        let mut encoder = Encoder::new(config, noop_handler())?;

        encoder.next_input_pts = i64::MAX;

        // new_timescale > old_timescale となる組合せで `i64::MAX * 2 / 1` が i64 範囲を超える。
        let err = encoder
            .reconfigure(ReconfigureParams {
                expected_frame_rate: Some(2),
                ..Default::default()
            })
            .expect_err("再スケールはオーバーフローすること");
        assert!(matches!(
            err,
            Error::LimitExceeded { reason } if reason == "rescaled presentation timestamp overflow",
        ));
        // 失敗時には config も next_input_pts も変更されない
        assert_eq!(encoder.config().fps_numerator, 1);
        assert_eq!(encoder.next_input_pts, i64::MAX);
        Ok(())
    }

    #[test]
    fn encode_rejects_pts_overflow_before_frame_submit() -> Result<(), Error> {
        // コールバックが発火しないことの検証は「一定時間待ってカウント 0」で行うため、
        // 陽性対照（正常フレーム 1 枚を送ってコールバックが届くこと）を併設する。
        let callback_count = Arc::new(AtomicUsize::new(0));
        let handler = FnEncodeHandler::new({
            let callback_count = Arc::clone(&callback_count);
            move |_: Result<EncodedFrame<()>, Error>| {
                callback_count.fetch_add(1, Ordering::SeqCst);
            }
        });

        let mut encoder = Encoder::new(base_encoder_config(), handler)?;
        let (y, u, v) = black_i420_frame(960, 480);
        let frame = FrameData::I420 {
            y: &y,
            u: &u,
            v: &v,
        };

        // 陽性対照: 正常フレームは送信され、コールバックが届くことを確認する。
        // 既存テストと同じく `finish()` で出力をフラッシュしてから待つ。
        encoder.encode(&frame, &EncodeOptions::default(), ())?;
        encoder.finish()?;
        wait_for_encode_callbacks(&callback_count, 1);
        let baseline = callback_count.load(Ordering::SeqCst);

        assert_pts_overflow_rejected(
            &mut encoder,
            |encoder| encoder.encode(&frame, &EncodeOptions::default(), ()),
            &callback_count,
            baseline,
        );

        Ok(())
    }

    #[test]
    fn encode_pixel_buffer_rejects_pts_overflow_before_frame_submit() -> Result<(), Error> {
        let callback_count = Arc::new(AtomicUsize::new(0));
        let handler = FnEncodeHandler::new({
            let callback_count = Arc::clone(&callback_count);
            move |_: Result<EncodedFrame<()>, Error>| {
                callback_count.fetch_add(1, Ordering::SeqCst);
            }
        });

        let mut encoder = Encoder::new(base_encoder_config(), handler)?;

        // 有効な CVPixelBuffer を生成する (I420 の 4:2:0 プラナー)
        let mut image_buffer = std::ptr::null_mut();
        let status = unsafe {
            sys::CVPixelBufferCreate(
                std::ptr::null_mut(),
                960,
                480,
                sys::kCVPixelFormatType_420YpCbCr8Planar,
                std::ptr::null(),
                &mut image_buffer,
            )
        };
        Error::check(status, "CVPixelBufferCreate")?;
        let image_buffer = CfPtrMut(image_buffer);

        // 陽性対照: 正常フレームは送信され、コールバックが届くことを確認する。
        // 既存テストと同じく `finish()` で出力をフラッシュしてから待つ。
        unsafe {
            encoder.encode_pixel_buffer(image_buffer.0.cast(), &EncodeOptions::default(), ())?;
        }
        encoder.finish()?;
        wait_for_encode_callbacks(&callback_count, 1);
        let baseline = callback_count.load(Ordering::SeqCst);

        assert_pts_overflow_rejected(
            &mut encoder,
            |encoder| unsafe {
                encoder.encode_pixel_buffer(image_buffer.0.cast(), &EncodeOptions::default(), ())
            },
            &callback_count,
            baseline,
        );

        Ok(())
    }

    /// エンコード開始後に `kVTCompressionPropertyKey_DataRateLimits` を変更しても
    /// 出力レートが変わらないことを検証する
    ///
    /// Video Toolbox はエンコード開始後の `DataRateLimits` の変更を無視するため、本クレートは
    /// `data_rate_limits` を構築時 (`EncoderConfig`) 専用にしている。本テストはその前提が
    /// 崩れたことを検出するためのもので、失敗した場合はエンコード中の変更が反映されるように
    /// なったことを意味する (その場合は `data_rate_limits` の扱いを見直すこと)。
    /// 陽性対照として、構築時に設定した上限が効くことも同じ条件で確認する。
    #[test]
    fn data_rate_limits_mid_stream_change_has_no_effect() -> Result<(), Error> {
        // 陽性対照: 構築時に設定した上限はウィンドウ合計を上限近傍まで抑える
        let capped = data_rate_limit_windowed_output(true)?;
        for (i, bytes) in capped.iter().enumerate().skip(1) {
            assert!(
                *bytes <= LIMIT_BYTES_PER_SEC * 150 / 100,
                "構築時に設定した上限が効いていない (ウィンドウ {i}): {bytes} バイト"
            );
        }

        // 検証対象: エンコード開始後に設定した上限は反映されない (上限なしと同じ出力になる)
        let mid_stream = data_rate_limit_windowed_output(false)?;
        for (i, bytes) in mid_stream.iter().enumerate().skip(1) {
            assert!(
                *bytes > LIMIT_BYTES_PER_SEC * 150 / 100,
                "エンコード開始後に設定した上限が効くようになった (ウィンドウ {i}): {bytes} バイト"
            );
        }
        Ok(())
    }

    /// 出力コールバックが `Err` を通知したときに `total_error_count` が増え、
    /// `in_flight_frames` が減ることを検証する
    ///
    /// Video Toolbox のフレームドロップは実機で確実に再現できないため、フレームドロップと
    /// 同じ「status は成功だが sample_buffer が NULL」の条件で出力コールバックを直接呼び出す。
    /// この経路はエラーとして利用側へ通知し、in-flight のフレームとしては完了扱いにする。
    #[test]
    fn output_callback_counts_error_and_decrements_in_flight() -> Result<(), Error> {
        let encoder = Encoder::new(base_encoder_config(), noop_handler())?;
        // 送信済み 1 件ぶんの in-flight を再現する
        encoder.context.stats.in_flight_frames.inc();

        let context_ptr: *const EncodeCallbackContext<FnEncodeHandler<()>> =
            encoder.context.as_ref();
        let user_data_ptr = Box::into_raw(Box::new(()));
        unsafe {
            Encoder::<FnEncodeHandler<()>>::output_callback_h264(
                context_ptr.cast_mut().cast::<c_void>(),
                user_data_ptr.cast::<c_void>(),
                0,
                0,
                std::ptr::null_mut(),
            );
        }

        let stats = encoder.stats();
        assert_eq!(
            stats.total_output_frame_count.get(),
            0,
            "出力データが無いフレームは出力フレーム数に計上しないこと"
        );
        assert_eq!(
            stats.total_error_count.get(),
            1,
            "sample_buffer が NULL のフレームはエラーとして計上すること"
        );
        assert_eq!(
            stats.in_flight_frames.get(),
            0,
            "コールバックが来たフレームは in-flight から外れること"
        );
        Ok(())
    }
}
