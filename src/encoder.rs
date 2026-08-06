//! H.264 / H.265 エンコーダー

mod callback;
mod config;
mod frame;
mod handler;
mod pixel_buffer;
mod session;
mod validation;

pub use config::{
    CodecConfig, DataRateLimit, EncodeOptions, EncoderConfig, H264EncoderConfig, H264EntropyMode,
    H264Profile, HevcEncoderConfig, HevcProfile, ReconfigureParams,
};
pub use frame::{EncodedFrame, FrameData};
pub use handler::{EncodeHandler, FnEncodeHandler};

use std::ffi::c_void;

use crate::{
    encoder::session::{
        push_bitrate_property, push_data_rate_limits_property, push_expected_frame_rate_property,
    },
    error::Error,
    sys,
    types::{CfPtr, cf_dictionary},
};

/// H.264 / H.265 エンコーダー
///
/// エンコード完了時に [`EncodeHandler::on_encoded`] を呼び出す。
/// この [`EncodeHandler::on_encoded`] の呼び出しは Video Toolbox のコールバックスレッドから行われる。
pub struct Encoder<H: EncodeHandler> {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    // FFI の outputCallbackRefCon にこの Box の中身ポインタを渡しているため、
    // Encoder の生存期間中は保持し続ける必要がある。Rust 側からは直接参照しない。
    #[expect(
        dead_code,
        reason = "FFI コールバックが Box の中身を借用するので Rust からは触らない"
    )]
    handler: Box<H>,
}

impl<H: EncodeHandler> Encoder<H> {
    /// エンコーダーのインスタンスを生成する
    pub fn new(mut config: EncoderConfig, handler: H) -> Result<Self, Error> {
        Self::validate_config(&config)?;
        // `Some(空 Vec)` は未設定と同義なので `None` へ正規化し、`config()` が返す表現を一意にする
        if config
            .data_rate_limits
            .as_deref()
            .is_some_and(<[_]>::is_empty)
        {
            config.data_rate_limits = None;
        }
        let handler = Box::new(handler);
        let session = unsafe { Self::create_compression_session(&config, handler.as_ref())? };

        Ok(Self {
            session,
            config,
            next_input_pts: 0,
            handler,
        })
    }

    /// 現在エンコーダーが内部で保持している設定を返す
    ///
    /// 戻り値は [`Encoder::new`] で渡した値、または直近の [`Encoder::reconfigure`] 呼び出しで
    /// 反映された値である。Video Toolbox がバックエンドで丸めた実効値とは異なる場合がある。
    ///
    /// [`Encoder::reconfigure`] 経由で動的に更新され得るのは [`ReconfigureParams`] に
    /// 対応するフィールドのみである。`average_bitrate` / `data_rate_limits` は同名の
    /// フィールドが、`fps_numerator` / `fps_denominator` は `expected_frame_rate` の指定に
    /// よって書き換わる。その他のフィールドは [`Encoder::new`] で渡した初期値のまま保持される。
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
            if let Some(ref limits) = params.data_rate_limits {
                push_data_rate_limits_property(&mut properties, &mut cf_objects, limits)?;
            }

            // 更新対象が無ければ no-op (パラメータのフィールド列挙で判定するとフィールド追加時に漏れる)
            if properties.is_empty() {
                return Ok(());
            }

            let properties_dict = cf_dictionary(&properties)?;
            let status = sys::VTSessionSetProperties(self.session.cast(), properties_dict.0.cast());
            Error::check(status, "VTSessionSetProperties")?;
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
        if let Some(limits) = params.data_rate_limits {
            // 解除 (空 Vec) は「未設定」へ正規化し、`config()` の表現を一意にする
            self.config.data_rate_limits = if limits.is_empty() {
                None
            } else {
                Some(limits)
            };
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
// handler は Box<H> でヒープに隔離されており、Encoder の生存期間中はアドレス不変である。
unsafe impl<H: EncodeHandler> Send for Encoder<H> {}

#[cfg(test)]
mod tests {
    //! `Encoder::reconfigure` の内部状態 (`next_input_pts`) を直接確認するためのテスト。
    //! `tests/test_encoder.rs` からは到達できない private フィールド検証だけをここに置く。
    //! 通常系の検証は `tests/test_encoder.rs` 側にある。

    use super::*;
    use crate::PixelFormat;

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
            data_rate_limits: None,
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
            .expect_err("rescale should overflow");
        assert!(matches!(
            err,
            Error::LimitExceeded { reason } if reason == "rescaled presentation timestamp overflow",
        ));
        // 失敗時には config も next_input_pts も変更されない
        assert_eq!(encoder.config().fps_numerator, 1);
        assert_eq!(encoder.next_input_pts, i64::MAX);
        Ok(())
    }
}
