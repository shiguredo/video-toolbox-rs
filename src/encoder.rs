use std::{ffi::c_void, num::NonZeroU32, time::Duration};

use crate::{
    error::Error,
    sys::{self, VTCompressionSessionCreate},
    types::{
        CfPtr, CfPtrMut, CvPixelBufferUnlockGuard, PixelFormat, cf_dictionary, cf_number_f64,
        cf_number_i32, cf_number_i64, validate_video_dimensions_for_toolbox,
    },
};

/// H.264 プロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Profile {
    /// Baseline (最高速)
    Baseline,
    /// Main
    Main,
    /// High (高品質)
    High,
}

/// H.264 エントロピー符号化モード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264EntropyMode {
    /// CAVLC (高速)
    Cavlc,
    /// CABAC (高品質)
    Cabac,
}

/// H.264 エンコーダー固有の設定
#[derive(Debug, Clone)]
pub struct H264EncoderConfig {
    /// kVTCompressionPropertyKey_ProfileLevel
    pub profile: H264Profile,
    /// kVTCompressionPropertyKey_H264EntropyMode
    pub entropy_mode: H264EntropyMode,
}

/// HEVC プロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HevcProfile {
    /// Main
    Main,
    /// Main10
    Main10,
}

/// HEVC エンコーダー固有の設定
#[derive(Debug, Clone)]
pub struct HevcEncoderConfig {
    /// kVTCompressionPropertyKey_ProfileLevel
    pub profile: HevcProfile,
    /// kVTCompressionPropertyKey_AllowOpenGOP
    pub allow_open_gop: bool,
}

/// コーデック固有の設定
#[derive(Debug, Clone)]
pub enum CodecConfig {
    /// H.264
    H264(H264EncoderConfig),
    /// HEVC (H.265)
    Hevc(HevcEncoderConfig),
}

/// エンコーダーに指定する設定
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// VTCompressionSessionCreate の width 引数
    pub width: u32,

    /// VTCompressionSessionCreate の height 引数
    pub height: u32,

    /// VTCompressionSessionCreate の codecType 引数およびコーデック固有の設定
    pub codec: CodecConfig,

    /// 入力ピクセルフォーマット
    ///
    /// Video Toolbox は CVPixelBuffer 単位でピクセルフォーマットを持つため、
    /// API レベルではフレームごとに異なるフォーマットを渡すことが可能である。
    /// しかし、エンコードセッション中にフォーマットが混在する使い方は一般的ではないため、
    /// このライブラリではセッション全体でフォーマットを固定し、
    /// `Encoder::encode()` で `FrameData` のバリアントとの整合性を検証する。
    pub pixel_format: PixelFormat,

    /// kVTCompressionPropertyKey_AverageBitRate (bps 単位、未指定時はバックエンド依存)
    pub average_bitrate: Option<u64>,

    /// kVTCompressionPropertyKey_ExpectedFrameRate の分子 (CMTimeMake の timescale としても使用)
    pub fps_numerator: u32,

    /// kVTCompressionPropertyKey_ExpectedFrameRate の分母 (CMTimeMake のフレーム間隔としても使用)
    pub fps_denominator: u32,

    /// kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality
    pub prioritize_encoding_speed_over_quality: bool,

    /// kVTCompressionPropertyKey_RealTime
    pub real_time: bool,

    /// kVTCompressionPropertyKey_MaximizePowerEfficiency
    pub maximize_power_efficiency: bool,

    /// kVTCompressionPropertyKey_AllowFrameReordering
    pub allow_frame_reordering: bool,

    /// kVTCompressionPropertyKey_AllowTemporalCompression
    pub allow_temporal_compression: bool,

    /// kVTCompressionPropertyKey_MaxKeyFrameInterval
    pub max_key_frame_interval: Option<NonZeroU32>,

    /// kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration
    pub max_key_frame_interval_duration: Option<Duration>,

    /// kVTCompressionPropertyKey_MaxFrameDelayCount
    pub max_frame_delay_count: Option<NonZeroU32>,
}

/// VTCompressionSessionEncodeFrame の frameProperties に指定するオプション
#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    /// kVTEncodeFrameOptionKey_ForceKeyFrame
    pub force_key_frame: bool,
}

/// [`Encoder::reconfigure`] で動的に更新可能なエンコードパラメータ
///
/// `None` のフィールドは現在値を維持する。全項目 `None` の場合は no-op となる。
///
/// 解像度・コーデック・ピクセルフォーマットなど Video Toolbox が動的変更をサポートしない
/// 項目はここに含まれていない。これらを変更する場合は [`Encoder`] を作り直す。
///
/// 将来のフィールド追加 (例: `DataRateLimits`) は破壊的変更として扱う。
/// 構築時は `ReconfigureParams::default()` を起点に必要なフィールドだけ更新する記述を推奨する。
#[derive(Debug, Clone, Default)]
pub struct ReconfigureParams {
    /// kVTCompressionPropertyKey_AverageBitRate (bps 単位)
    pub average_bitrate: Option<u64>,

    /// kVTCompressionPropertyKey_ExpectedFrameRate (整数 fps)
    ///
    /// 詳細な正規化 / 再スケール挙動は [`Encoder::reconfigure`] の rustdoc を参照。
    pub expected_frame_rate: Option<u32>,
}

// パラメータセット (VPS, SPS, PPS) のタプル型
type ParameterSets = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// `average_bitrate` (bps) の境界を検証する
fn validate_average_bitrate(bitrate: u64) -> Result<(), Error> {
    if bitrate == 0 {
        return Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must not be zero",
        });
    }
    if bitrate > i64::MAX as u64 {
        return Err(Error::InvalidConfig {
            field: "average_bitrate",
            reason: "must fit in i64 for CFNumber",
        });
    }
    Ok(())
}

/// `fps_numerator` の境界を検証する (CMTimeMake の timescale 用)
fn validate_fps_numerator(value: u32) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must not be zero",
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field: "fps_numerator",
            reason: "must fit in i32 for CMTime timescale",
        });
    }
    Ok(())
}

/// `expected_frame_rate` の境界を検証する
fn validate_expected_frame_rate(value: u32) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must not be zero",
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field: "expected_frame_rate",
            reason: "must fit in i32 for CFNumber",
        });
    }
    Ok(())
}

/// エンコード結果を通知するためのハンドラー
///
/// エンコード処理が完了するたびに [`EncodeHandler::on_encoded`] が呼ばれる。
pub trait EncodeHandler: Send + 'static {
    /// ユーザーデータ型
    type UserData: Send + 'static;
    /// エラー型
    type Error: From<crate::Error> + Send + 'static;
    /// エンコード完了時に呼ばれる
    fn on_encoded(&mut self, result: Result<EncodedFrame<Self::UserData>, Self::Error>);
}

/// `FnMut(Result<EncodedFrame<T>, E>)` を [`EncodeHandler`] にするラッパー
pub struct FnEncodeHandler<T, E = crate::Error> {
    f: Box<dyn FnMut(Result<EncodedFrame<T>, E>) + Send + 'static>,
}

impl<T, E> FnEncodeHandler<T, E> {
    /// `FnMut(Result<EncodedFrame<T>, E>)` から [`EncodeHandler`] を構築する
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(Result<EncodedFrame<T>, E>) + Send + 'static,
    {
        Self { f: Box::new(f) }
    }
}

impl<T, E> EncodeHandler for FnEncodeHandler<T, E>
where
    T: Send + 'static,
    E: From<crate::Error> + Send + 'static,
{
    type UserData = T;
    type Error = E;
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, E>) {
        (self.f)(result);
    }
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
    // Encoder の生存期間中は保持し続ける必要がある。Rust 側からは直接参照しない。
    #[allow(dead_code)]
    handler: Box<H>,
}

impl<H: EncodeHandler> Encoder<H> {
    /// エンコーダーのインスタンスを生成する
    pub fn new(config: EncoderConfig, handler: H) -> Result<Self, Error> {
        Self::validate_config(&config)?;
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
    /// [`Encoder::reconfigure`] 経由で動的に更新され得るのは `average_bitrate` /
    /// `fps_numerator` / `fps_denominator` の 3 項目のみで、その他のフィールドは
    /// [`Encoder::new`] で渡した初期値のまま保持される。
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
    /// `expected_frame_rate` を更新した場合は、内部の `next_input_pts` を新しい timescale に切り上げで
    /// 再スケールし、直前出力フレームと物理時間として単調増加するようにする。再スケール結果が
    /// `i64` を超えた場合は [`Error::LimitExceeded`] を返す。
    ///
    /// 解像度・コーデック・ピクセルフォーマットの変更はこの API では対応しないため [`Encoder`] を作り直す。
    pub fn reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error> {
        // 全項目 `None` のときは API 呼び出し自体を省略する
        if params.average_bitrate.is_none() && params.expected_frame_rate.is_none() {
            return Ok(());
        }

        Self::validate_reconfigure_params(&params)?;

        // VTSessionSetProperties 実行前に `next_input_pts` の再スケール値を確定させる。
        // FFI 呼び出しが失敗した場合はここまでで巻き戻し、self の状態を変更しない。
        // 直前出力フレームとの PTS 単調性を維持するため切り上げ (div_ceil) で再スケールする。
        // 切り捨てだと new_timescale < old_timescale 時に rescaled が潰れて逆行し得る。
        let rescaled_next_input_pts = if let Some(fps) = params.expected_frame_rate {
            let old_timescale = self.config.fps_numerator as i128;
            let new_timescale = fps as i128;
            let old_pts = self.next_input_pts as i128;
            let product = old_pts * new_timescale;
            let rescaled = (product + old_timescale - 1) / old_timescale;
            if !(i64::MIN as i128..=i64::MAX as i128).contains(&rescaled) {
                return Err(Error::LimitExceeded {
                    reason: "rescaled presentation timestamp overflow",
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
                let value = cf_number_i64(bitrate as i64)?;
                properties.push((sys::kVTCompressionPropertyKey_AverageBitRate, value.0));
                cf_objects.push(value);
            }
            if let Some(fps) = params.expected_frame_rate {
                let value = cf_number_i32(fps as i32)?;
                properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, value.0));
                cf_objects.push(value);
            }

            let properties_dict = cf_dictionary(&properties)?;
            let _properties_dict_guard = CfPtr(properties_dict.cast::<c_void>());
            let status = sys::VTSessionSetProperties(self.session.cast(), properties_dict);
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
        if let Some(new_pts) = rescaled_next_input_pts {
            self.next_input_pts = new_pts;
        }

        Ok(())
    }

    /// EncoderConfig と完了コールバックから VTCompressionSession を作成する
    unsafe fn create_compression_session(
        config: &EncoderConfig,
        handler: &H,
    ) -> Result<sys::VTCompressionSessionRef, Error> {
        unsafe {
            let mut session = std::ptr::null_mut();

            let (codec_fourcc, callback, profile_level) = match &config.codec {
                CodecConfig::Hevc(hevc) => {
                    let profile_level = match hevc.profile {
                        HevcProfile::Main => sys::kVTProfileLevel_HEVC_Main_AutoLevel,
                        HevcProfile::Main10 => sys::kVTProfileLevel_HEVC_Main10_AutoLevel,
                    };
                    (
                        u32::from_be_bytes(*b"hvc1"),
                        Self::output_callback_h265 as unsafe extern "C" fn(_, _, _, _, _),
                        profile_level,
                    )
                }
                CodecConfig::H264(h264) => {
                    let profile_level = match h264.profile {
                        H264Profile::Baseline => sys::kVTProfileLevel_H264_Baseline_AutoLevel,
                        H264Profile::Main => sys::kVTProfileLevel_H264_Main_AutoLevel,
                        H264Profile::High => sys::kVTProfileLevel_H264_High_AutoLevel,
                    };
                    (
                        u32::from_be_bytes(*b"avc1"),
                        Self::output_callback_h264 as unsafe extern "C" fn(_, _, _, _, _),
                        profile_level,
                    )
                }
            };

            // SAFETY:
            // - `outputCallbackRefCon` には `&H` のポインタを渡す。
            //   `handler` は `Box<H>` でヒープに隔離されており、`Encoder` の生存期間中はアドレス不変である。
            // - 出力コールバック内で `&mut H` として復元し、ユーザー指定のハンドラを呼び出す (`process_encoded_output`)。
            let status = VTCompressionSessionCreate(
                std::ptr::null_mut(),
                config.width as i32,
                config.height as i32,
                codec_fourcc,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(callback),
                (handler as *const H).cast::<c_void>().cast_mut(),
                &mut session,
            );
            Error::check(status, "VTCompressionSessionCreate")?;

            // 共通のプロパティ設定
            let mut properties = Vec::new();
            let mut cf_objects: Vec<CfPtr<c_void>> = Vec::new();
            Self::add_common_properties(&mut properties, &mut cf_objects, config)?;

            // プロファイルレベル設定
            properties.push((
                sys::kVTCompressionPropertyKey_ProfileLevel,
                profile_level.cast(),
            ));

            // コーデック固有の設定
            match &config.codec {
                CodecConfig::Hevc(hevc) => {
                    Self::add_h265_specific_properties(&mut properties, hevc)?;
                }
                CodecConfig::H264(h264) => {
                    Self::add_h264_specific_properties(&mut properties, h264)?;
                }
            }

            let properties_dict = cf_dictionary(&properties)?;
            let _properties_dict_guard = CfPtr(properties_dict.cast::<c_void>());
            let status = sys::VTSessionSetProperties(session.cast(), properties_dict);
            Error::check(status, "VTSessionSetProperties")?;

            Ok(session)
        }
    }

    /// 共通のプロパティを追加
    fn add_common_properties(
        properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
        cf_objects: &mut Vec<CfPtr<c_void>>,
        config: &EncoderConfig,
    ) -> Result<(), Error> {
        unsafe {
            // 基本設定
            let fps = cf_number_i32(config.fps_numerator.div_ceil(config.fps_denominator) as i32)?;

            // ビットレート (指定時のみ設定)
            if let Some(bitrate) = config.average_bitrate {
                let target_bitrate = cf_number_i64(bitrate as i64)?;
                properties.push((
                    sys::kVTCompressionPropertyKey_AverageBitRate,
                    target_bitrate.0,
                ));
                cf_objects.push(target_bitrate);
            }

            properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, fps.0));
            cf_objects.push(fps);

            // リアルタイムモード
            properties.push((
                sys::kVTCompressionPropertyKey_RealTime,
                if config.real_time {
                    sys::kCFBooleanTrue
                } else {
                    sys::kCFBooleanFalse
                }
                .cast(),
            ));

            // フレーム再順序付け
            properties.push((
                sys::kVTCompressionPropertyKey_AllowFrameReordering,
                if config.allow_frame_reordering {
                    sys::kCFBooleanTrue
                } else {
                    sys::kCFBooleanFalse
                }
                .cast(),
            ));

            // 時間的圧縮
            properties.push((
                sys::kVTCompressionPropertyKey_AllowTemporalCompression,
                if config.allow_temporal_compression {
                    sys::kCFBooleanTrue
                } else {
                    sys::kCFBooleanFalse
                }
                .cast(),
            ));

            // キーフレーム間隔（フレーム数）
            if let Some(interval) = config.max_key_frame_interval {
                let interval_value = cf_number_i32(interval.get() as i32)?;
                properties.push((
                    sys::kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    interval_value.0,
                ));
                cf_objects.push(interval_value);
            }

            // キーフレーム間隔（秒数）
            if let Some(duration) = config.max_key_frame_interval_duration {
                let duration_value = cf_number_f64(duration.as_secs_f64())?;
                properties.push((
                    sys::kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
                    duration_value.0,
                ));
                cf_objects.push(duration_value);
            }

            // フレーム遅延制限
            if let Some(delay_count) = config.max_frame_delay_count {
                let delay_value = cf_number_i32(delay_count.get() as i32)?;
                properties.push((
                    sys::kVTCompressionPropertyKey_MaxFrameDelayCount,
                    delay_value.0,
                ));
                cf_objects.push(delay_value);
            }

            // 速度優先モード
            if config.prioritize_encoding_speed_over_quality {
                properties.push((
                    sys::kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
                    sys::kCFBooleanTrue.cast(),
                ));
            }

            // 電力効率最大化
            if config.maximize_power_efficiency {
                properties.push((
                    sys::kVTCompressionPropertyKey_MaximizePowerEfficiency,
                    sys::kCFBooleanTrue.cast(),
                ));
            }
        }
        Ok(())
    }

    /// H.264 固有のプロパティを追加
    fn add_h264_specific_properties(
        properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
        h264: &H264EncoderConfig,
    ) -> Result<(), Error> {
        unsafe {
            // H.264 エントロピー符号化モード
            let entropy_mode = match h264.entropy_mode {
                H264EntropyMode::Cavlc => sys::kVTH264EntropyMode_CAVLC,
                H264EntropyMode::Cabac => sys::kVTH264EntropyMode_CABAC,
            };
            properties.push((
                sys::kVTCompressionPropertyKey_H264EntropyMode,
                entropy_mode.cast(),
            ));
        }
        Ok(())
    }

    /// H.265 固有のプロパティを追加
    fn add_h265_specific_properties(
        properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
        hevc: &HevcEncoderConfig,
    ) -> Result<(), Error> {
        unsafe {
            // Open GOP 設定（H.265 のみ）
            if !hevc.allow_open_gop {
                properties.push((
                    sys::kVTCompressionPropertyKey_AllowOpenGOP,
                    sys::kCFBooleanFalse.cast(),
                ));
            }
        }
        Ok(())
    }

    /// エンコーダー設定を検証する
    fn validate_config(config: &EncoderConfig) -> Result<(), Error> {
        validate_video_dimensions_for_toolbox(config.width, config.height)?;
        if config.fps_denominator == 0 {
            return Err(Error::InvalidConfig {
                field: "fps_denominator",
                reason: "must not be zero",
            });
        }
        validate_fps_numerator(config.fps_numerator)?;
        if let Some(bitrate) = config.average_bitrate {
            validate_average_bitrate(bitrate)?;
        }
        Ok(())
    }

    /// [`Encoder::reconfigure`] に渡された [`ReconfigureParams`] を検証する
    fn validate_reconfigure_params(params: &ReconfigureParams) -> Result<(), Error> {
        if let Some(bitrate) = params.average_bitrate {
            validate_average_bitrate(bitrate)?;
        }
        if let Some(fps) = params.expected_frame_rate {
            validate_expected_frame_rate(fps)?;
        }
        Ok(())
    }

    /// 入力プレーンデータを CVPixelBuffer のプレーンにコピーする
    ///
    /// CVPixelBuffer のストライド（bytes_per_row）が入力幅より大きい場合は
    /// 行ごとにコピーする。
    unsafe fn copy_plane(
        pixel_buffer: sys::CVPixelBufferRef,
        plane_index: usize,
        src: &[u8],
        src_width: usize,
        src_height: usize,
    ) -> Result<(), Error> {
        unsafe {
            let dst = sys::CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, plane_index) as *mut u8;
            if dst.is_null() {
                return Err(Error::LimitExceeded {
                    reason: "CVPixelBuffer base address for plane is null",
                });
            }
            let dst_stride = sys::CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, plane_index);
            let cv_plane_height = sys::CVPixelBufferGetHeightOfPlane(pixel_buffer, plane_index);
            let cv_plane_width = sys::CVPixelBufferGetWidthOfPlane(pixel_buffer, plane_index);
            // 行あたり `dst_stride` バイトしかないのに `src_width` バイトを書くとバッファ外になる
            if dst_stride < src_width {
                return Err(Error::LimitExceeded {
                    reason: "plane destination stride is less than copy width",
                });
            }
            // Core Video の契約では、プレーンは少なくとも `height * bytesPerRow` バイトを指す。
            // コピー範囲が `CVPixelBuffer` が報告するプレーン寸法を超えないことを検証する。
            if src_width > cv_plane_width || src_height > cv_plane_height {
                return Err(Error::LimitExceeded {
                    reason: "plane copy dimensions exceed CVPixelBuffer plane bounds",
                });
            }
            let plane_storage =
                dst_stride
                    .checked_mul(cv_plane_height)
                    .ok_or(Error::LimitExceeded {
                        reason: "plane storage byte length overflow",
                    })?;
            let write_span = if src_height == 0 {
                0
            } else if dst_stride == src_width {
                src_width
                    .checked_mul(src_height)
                    .ok_or(Error::LimitExceeded {
                        reason: "plane copy byte length overflow",
                    })?
            } else {
                src_height
                    .checked_sub(1)
                    .and_then(|r| r.checked_mul(dst_stride))
                    .and_then(|o| o.checked_add(src_width))
                    .ok_or(Error::LimitExceeded {
                        reason: "plane row copy span overflow",
                    })?
            };
            if write_span > plane_storage {
                return Err(Error::LimitExceeded {
                    reason: "plane copy would exceed CVPixelBuffer plane storage",
                });
            }

            let copy_size = src_width
                .checked_mul(src_height)
                .ok_or(Error::LimitExceeded {
                    reason: "plane copy byte length overflow",
                })?;
            if dst_stride == src_width {
                // ストライドと入力幅が一致する場合は一括コピー
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst, copy_size);
            } else {
                // ストライドが異なる場合は行ごとにコピー
                for row in 0..src_height {
                    let src_off = row.checked_mul(src_width).ok_or(Error::LimitExceeded {
                        reason: "plane row source offset overflow",
                    })?;
                    let dst_off = row.checked_mul(dst_stride).ok_or(Error::LimitExceeded {
                        reason: "plane row destination offset overflow",
                    })?;
                    std::ptr::copy_nonoverlapping(
                        src.as_ptr().add(src_off),
                        dst.add(dst_off),
                        src_width,
                    );
                }
            }
            Ok(())
        }
    }

    /// フレームデータの長さが必要なサイズを満たしているか検証する
    fn validate_frame_data(
        frame: &FrameData<'_>,
        width: usize,
        height: usize,
    ) -> Result<(), Error> {
        match frame {
            FrameData::I420 { y, u, v } => {
                let y_expected = Self::frame_byte_len_checked(width, height)?;
                let uv_w = width.div_ceil(2);
                let uv_h = height.div_ceil(2);
                let uv_expected = Self::frame_byte_len_checked(uv_w, uv_h)?;
                if y.len() < y_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "Y",
                        expected: y_expected,
                        actual: y.len(),
                    });
                }
                if u.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "U",
                        expected: uv_expected,
                        actual: u.len(),
                    });
                }
                if v.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "V",
                        expected: uv_expected,
                        actual: v.len(),
                    });
                }
            }
            FrameData::Nv12 { y, uv } => {
                let y_expected = Self::frame_byte_len_checked(width, height)?;
                let uv_expected = Self::frame_byte_len_checked(width, height.div_ceil(2))?;
                if y.len() < y_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "Y",
                        expected: y_expected,
                        actual: y.len(),
                    });
                }
                if uv.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "UV",
                        expected: uv_expected,
                        actual: uv.len(),
                    });
                }
            }
        }
        Ok(())
    }

    /// `width * height` 等のフレームサイズ計算で `usize` 乗算がオーバーフローしないことを保証する
    fn frame_byte_len_checked(a: usize, b: usize) -> Result<usize, Error> {
        a.checked_mul(b).ok_or(Error::LimitExceeded {
            reason: "frame dimension size overflow",
        })
    }

    /// 画像データをエンコードする
    ///
    /// エンコード結果は構築時に指定したコールバックで通知される
    ///
    /// なお `y` のストライドは入力フレームの幅と等しいことが前提
    pub fn encode(
        &mut self,
        frame: &FrameData<'_>,
        options: &EncodeOptions,
        user_data: H::UserData,
    ) -> Result<(), Error> {
        // Video Toolbox は CVPixelBuffer 単位でフォーマットを持つためフレームごとに変更可能だが、
        // このライブラリでは EncoderConfig.pixel_format でセッション全体のフォーマットを固定している。
        // FrameData のバリアントが EncoderConfig.pixel_format と一致しない場合はエラーとする。
        let actual = match frame {
            FrameData::I420 { .. } => PixelFormat::I420,
            FrameData::Nv12 { .. } => PixelFormat::Nv12,
        };
        if actual != self.config.pixel_format {
            return Err(Error::PixelFormatMismatch {
                expected: self.config.pixel_format,
                actual,
            });
        }

        let width = self.config.width as usize;
        let height = self.config.height as usize;

        // 入力データの長さを検証
        Self::validate_frame_data(frame, width, height)?;

        unsafe {
            // CVPixelBufferCreate で CoreVideo にメモリを確保させ、入力データをコピーする。
            // CVPixelBufferCreateWithPlanarBytes を使うと外部メモリへの参照を渡すことになり、
            // VTCompressionSessionEncodeFrame が非同期にデータを読む場合にメモリ寿命が保証されない。
            let pixel_format_type = match frame {
                FrameData::I420 { .. } => sys::kCVPixelFormatType_420YpCbCr8Planar,
                FrameData::Nv12 { .. } => sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            };

            let mut image_buffer = std::ptr::null_mut();
            let status = sys::CVPixelBufferCreate(
                std::ptr::null_mut(),
                width,
                height,
                pixel_format_type,
                std::ptr::null(),
                &mut image_buffer,
            );
            Error::check(status, "CVPixelBufferCreate")?;

            let image_buffer = CfPtrMut(image_buffer);

            // CPU でプレーンへ書き込む間だけロックし、`VTCompressionSessionEncodeFrame` 呼び出し前に解放する。
            // `copy_plane` が Err のときも `Drop` でアンロックする（`CfPtrMut` の `CFRelease` 前にロック残しを防ぐ）。
            {
                let status = sys::CVPixelBufferLockBaseAddress(image_buffer.0, 0);
                Error::check(status, "CVPixelBufferLockBaseAddress")?;
                let _pixel_unlock = CvPixelBufferUnlockGuard(image_buffer.0);
                match frame {
                    FrameData::I420 { y, u, v } => {
                        Self::copy_plane(image_buffer.0, 0, y, width, height)?;
                        Self::copy_plane(
                            image_buffer.0,
                            1,
                            u,
                            width.div_ceil(2),
                            height.div_ceil(2),
                        )?;
                        Self::copy_plane(
                            image_buffer.0,
                            2,
                            v,
                            width.div_ceil(2),
                            height.div_ceil(2),
                        )?;
                    }
                    FrameData::Nv12 { y, uv } => {
                        Self::copy_plane(image_buffer.0, 0, y, width, height)?;
                        Self::copy_plane(image_buffer.0, 1, uv, width, height.div_ceil(2))?;
                    }
                }
            }

            let frame_properties = if options.force_key_frame {
                cf_dictionary(&[(
                    sys::kVTEncodeFrameOptionKey_ForceKeyFrame,
                    sys::kCFBooleanTrue as *const c_void,
                )])?
            } else {
                std::ptr::null()
            };
            let _frame_properties_guard = if !frame_properties.is_null() {
                Some(CfPtr(frame_properties.cast::<c_void>()))
            } else {
                None
            };
            let source_frame_ref_con = Box::into_raw(Box::new(user_data)).cast::<c_void>();

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties,
                source_frame_ref_con,
                std::ptr::null_mut(),
            );
            if let Err(e) = Error::check(status, "VTCompressionSessionEncodeFrame") {
                // status エラー時は sourceFrameRefCon がコールバックされないため、ここで drop する
                let _ = Box::from_raw(source_frame_ref_con.cast::<H::UserData>());
                return Err(e);
            }

            self.next_input_pts = self
                .next_input_pts
                .checked_add(self.config.fps_denominator as i64)
                .ok_or(Error::LimitExceeded {
                    reason: "input presentation timestamp overflow",
                })?;

            Ok(())
        }
    }

    /// CVPixelBuffer を直接エンコードする（ゼロコピー）
    ///
    /// video-device-rs の `PixelBuffer::as_ptr()` で取得した CVPixelBuffer ポインタを渡す。
    /// 内部で CFRetain するため、呼び出し元はこの関数の後にポインタ元を drop してよい。
    ///
    /// エンコード結果は構築時に指定したコールバックで通知される
    ///
    /// # Safety
    ///
    /// `pixel_buffer_ptr` は有効な CVPixelBuffer ポインタでなければならない。
    /// また [`EncoderConfig`] の解像度・プレーン構成と整合するピクセルバッファであることは **呼び出し側の責務**とする。
    /// （本関数はピクセルフォーマットのみ検証し、幅・高さの不一致は即クラッシュしない場合があるが、エンコード結果は不正になりうる。）
    /// 本関数は `CVPixelBufferLockBaseAddress` を呼ばない。呼び出し側がロックしている場合、Video Toolbox の期待に合わせて **エンコード前にアンロック**するのは呼び出し側の責務とする。
    pub unsafe fn encode_pixel_buffer(
        &mut self,
        pixel_buffer_ptr: *mut c_void,
        options: &EncodeOptions,
        user_data: H::UserData,
    ) -> Result<(), Error> {
        unsafe {
            // ピクセルフォーマットの検証
            let format_type = sys::CVPixelBufferGetPixelFormatType(pixel_buffer_ptr.cast());
            let actual = match format_type {
                x if x == u32::from_be_bytes(*b"y420") => PixelFormat::I420,
                x if x == sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => PixelFormat::Nv12,
                _ => {
                    // 未知のフォーマットは I420 でも Nv12 でもないので、
                    // どちらを actual にしても不一致になる。期待値の逆を返す。
                    let actual = match self.config.pixel_format {
                        PixelFormat::I420 => PixelFormat::Nv12,
                        PixelFormat::Nv12 => PixelFormat::I420,
                    };
                    return Err(Error::PixelFormatMismatch {
                        expected: self.config.pixel_format,
                        actual,
                    });
                }
            };
            if actual != self.config.pixel_format {
                return Err(Error::PixelFormatMismatch {
                    expected: self.config.pixel_format,
                    actual,
                });
            }

            // CFRetain して CfPtrMut でラップ（スコープ終了時に CFRelease される）
            sys::CFRetain(pixel_buffer_ptr.cast());
            let image_buffer = CfPtrMut(pixel_buffer_ptr.cast::<sys::__CVBuffer>());

            let frame_properties = if options.force_key_frame {
                cf_dictionary(&[(
                    sys::kVTEncodeFrameOptionKey_ForceKeyFrame,
                    sys::kCFBooleanTrue as *const c_void,
                )])?
            } else {
                std::ptr::null()
            };
            let _frame_properties_guard = if !frame_properties.is_null() {
                Some(CfPtr(frame_properties.cast::<c_void>()))
            } else {
                None
            };
            let source_frame_ref_con = Box::into_raw(Box::new(user_data)).cast::<c_void>();

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties,
                source_frame_ref_con,
                std::ptr::null_mut(),
            );
            if let Err(e) = Error::check(status, "VTCompressionSessionEncodeFrame") {
                let _ = Box::from_raw(source_frame_ref_con.cast::<H::UserData>());
                return Err(e);
            }

            self.next_input_pts = self
                .next_input_pts
                .checked_add(self.config.fps_denominator as i64)
                .ok_or(Error::LimitExceeded {
                    reason: "input presentation timestamp overflow",
                })?;

            Ok(())
        }
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

    unsafe fn take_user_data(
        source_frame_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Option<H::UserData> {
        if source_frame_ref_con.is_null() {
            log::error!("{callback_name}: source_frame_ref_con is null");
            return None;
        }
        Some(unsafe { *Box::from_raw(source_frame_ref_con.cast::<H::UserData>()) })
    }

    unsafe fn callback_from_ref_con<'a>(
        output_callback_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Option<&'a mut H> {
        if output_callback_ref_con.is_null() {
            log::error!("{callback_name}: output_callback_ref_con is null");
            return None;
        }
        // SAFETY:
        // - `output_callback_ref_con` は `Box<H>` のヒープアドレスを指す。
        //   `Box<H>` のヒープアドレスは `Encoder` の生存期間中不変である。
        // - FFI コールバックは `&mut H` で排他的にアクセスする。
        Some(unsafe { &mut *output_callback_ref_con.cast::<H>() })
    }

    fn invoke_callback(handler: &mut H, result: Result<EncodedFrame<H::UserData>, H::Error>) {
        handler.on_encoded(result);
    }

    unsafe extern "C" fn output_callback_h264(
        output_callback_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
                source_frame_ref_con,
                sample_buffer,
                status,
                "output_callback_h264",
                Self::extract_h264_params,
            );
        }
    }

    unsafe extern "C" fn output_callback_h265(
        output_callback_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
                source_frame_ref_con,
                sample_buffer,
                status,
                "output_callback_h265",
                Self::extract_h265_params,
            );
        }
    }

    /// エンコードコールバックの共通処理
    unsafe fn process_encoded_output(
        output_callback_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        sample_buffer: sys::CMSampleBufferRef,
        status: i32,
        callback_name: &'static str,
        extract_params: unsafe fn(sys::CMVideoFormatDescriptionRef) -> Option<ParameterSets>,
    ) {
        let Some(user_data) =
            (unsafe { Self::take_user_data(source_frame_ref_con, callback_name) })
        else {
            return;
        };
        let Some(handler) =
            (unsafe { Self::callback_from_ref_con(output_callback_ref_con, callback_name) })
        else {
            return;
        };

        if let Err(e) = Error::check(status, callback_name) {
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        // フレームドロップ等で sample_buffer が NULL になる場合がある
        if sample_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "encoded sample buffer is null",
            };
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        unsafe {
            let data_buffer = sys::CMSampleBufferGetDataBuffer(sample_buffer);
            if data_buffer.is_null() {
                let e = Error::LimitExceeded {
                    reason: "CMSampleBufferGetDataBuffer returned null",
                };
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }
            // `CMBlockBufferGetDataPointer` の戻り長はオフセットからの連続領域長であり、ブロック全体長ではない。
            // 非連続バッファでは `data_pointer_len < block_len` になり得るため、`CMBlockBufferCopyDataBytes` で全長をコピーする。
            let block_len = sys::CMBlockBufferGetDataLength(data_buffer);
            if block_len > MAX_ENCODED_BLOCK_COPY_BYTES {
                let e = Error::LimitExceeded {
                    reason: "encoded block length exceeds defensive maximum",
                };
                log::error!(
                    "CMBlockBufferGetDataLength {block_len} exceeds defensive maximum {max}",
                    max = MAX_ENCODED_BLOCK_COPY_BYTES
                );
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }
            let mut data = vec![0u8; block_len];
            let status = sys::CMBlockBufferCopyDataBytes(
                data_buffer,
                0,
                block_len,
                data.as_mut_ptr().cast(),
            );
            if let Err(e) = Error::check(status, "CMBlockBufferCopyDataBytes") {
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }

            let description = sys::CMSampleBufferGetFormatDescription(sample_buffer);
            let keyframe = is_keyframe(sample_buffer);

            let (vps_list, sps_list, pps_list) = if keyframe {
                if description.is_null() {
                    let e = Error::LimitExceeded {
                        reason: "CMSampleBufferGetFormatDescription returned null for keyframe",
                    };
                    Self::invoke_callback(handler, Err(e.into()));
                    return;
                }
                match extract_params(description) {
                    Some(params) => params,
                    None => {
                        let e = Error::LimitExceeded {
                            reason: "failed to extract codec parameter sets",
                        };
                        Self::invoke_callback(handler, Err(e.into()));
                        return;
                    }
                }
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };

            let frame = EncodedFrame {
                keyframe,
                sps_list,
                pps_list,
                vps_list,
                data,
                user_data,
            };
            Self::invoke_callback(handler, Ok(frame));
        }
    }

    /// H.264 のパラメータセット (SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル (H.264 では vps_list は空)
    unsafe fn extract_h264_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Option<ParameterSets> {
        unsafe {
            if description.is_null() {
                log::error!("CMVideoFormatDescription is null in extract_h264_params");
                return None;
            }
            let mut nalu_header_length = 0;
            let status = sys::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                description,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut nalu_header_length,
            );
            if let Err(e) =
                Error::check(status, "CMVideoFormatDescriptionGetH264ParameterSetAtIndex")
            {
                log::error!("{e}");
                return None;
            }
            if nalu_header_length != 4 {
                log::error!("unexpected NAL unit header length: {nalu_header_length}");
                return None;
            }

            let mut sps_ptr = std::ptr::null();
            let mut pps_ptr = std::ptr::null();
            let mut sps_size = 0;
            let mut pps_size = 0;

            for (i, (ps_ptr, ps_size)) in
                [(&mut sps_ptr, &mut sps_size), (&mut pps_ptr, &mut pps_size)]
                    .into_iter()
                    .enumerate()
            {
                let status = sys::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    description,
                    i,
                    ps_ptr,
                    ps_size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if let Err(e) =
                    Error::check(status, "CMVideoFormatDescriptionGetH264ParameterSetAtIndex")
                {
                    log::error!("{e}");
                    return None;
                }
            }

            let sps_vec = vec_u8_from_raw_parts_safe(sps_ptr, sps_size, "H264 SPS")?;
            let pps_vec = vec_u8_from_raw_parts_safe(pps_ptr, pps_size, "H264 PPS")?;

            Some((Vec::new(), vec![sps_vec], vec![pps_vec]))
        }
    }

    /// H.265 のパラメータセット (VPS, SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル
    unsafe fn extract_h265_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Option<ParameterSets> {
        unsafe {
            if description.is_null() {
                log::error!("CMVideoFormatDescription is null in extract_h265_params");
                return None;
            }
            let mut nalu_header_length = 0;
            let status = sys::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                description,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut nalu_header_length,
            );
            if let Err(e) =
                Error::check(status, "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex")
            {
                log::error!("{e}");
                return None;
            }
            if nalu_header_length != 4 {
                log::error!("unexpected NAL unit header length: {nalu_header_length}");
                return None;
            }

            let mut vps_ptr = std::ptr::null();
            let mut sps_ptr = std::ptr::null();
            let mut pps_ptr = std::ptr::null();
            let mut vps_size = 0;
            let mut sps_size = 0;
            let mut pps_size = 0;

            for (i, (ps_ptr, ps_size)) in [
                (&mut vps_ptr, &mut vps_size),
                (&mut sps_ptr, &mut sps_size),
                (&mut pps_ptr, &mut pps_size),
            ]
            .into_iter()
            .enumerate()
            {
                let status = sys::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                    description,
                    i,
                    ps_ptr,
                    ps_size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if let Err(e) =
                    Error::check(status, "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex")
                {
                    log::error!("{e}");
                    return None;
                }
            }

            let vps_vec = vec_u8_from_raw_parts_safe(vps_ptr, vps_size, "HEVC VPS")?;
            let sps_vec = vec_u8_from_raw_parts_safe(sps_ptr, sps_size, "HEVC SPS")?;
            let pps_vec = vec_u8_from_raw_parts_safe(pps_ptr, pps_size, "HEVC PPS")?;

            Some((vec![vps_vec], vec![sps_vec], vec![pps_vec]))
        }
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

/// エンコードされた映像フレーム (AVCC 形式)
#[derive(Debug)]
pub struct EncodedFrame<T> {
    /// キーフレームかどうか
    pub keyframe: bool,

    /// SPS
    pub sps_list: Vec<Vec<u8>>,

    /// PPS
    pub pps_list: Vec<Vec<u8>>,

    /// VPS (H.265 only)
    pub vps_list: Vec<Vec<u8>>,

    /// 圧縮データ
    pub data: Vec<u8>,

    /// `encode` / `encode_pixel_buffer` 呼び出し時に指定したユーザーデータ
    pub user_data: T,
}

/// エンコーダーに渡すフレームデータ
pub enum FrameData<'a> {
    /// I420 (3 プレーン)
    I420 {
        /// Y プレーン
        y: &'a [u8],
        /// U プレーン
        u: &'a [u8],
        /// V プレーン
        v: &'a [u8],
    },
    /// NV12 (2 プレーン)
    Nv12 {
        /// Y プレーン
        y: &'a [u8],
        /// UV インターリーブプレーン
        uv: &'a [u8],
    },
}

/// パラメータセット 1 個あたりのコピー上限（バイト）。
///
/// **根拠（レビューで追試可能）**: ISO/IEC 14496-15（MPEG-4 Part 15）において、
/// `AVCDecoderConfigurationRecord` の `sequenceParameterSetLength` / `pictureParameterSetLength`、
/// および `HEVCDecoderConfigurationRecord` 内の各 NAL の長さは **`unsigned int(16)`** で表される。
/// よって 1 パラメータセットあたり表現可能な最大長は **65535 バイト**（`2^16 - 1`）である。
/// 本クレートが扱う AVCC 互換のパラメータセット長と整合する防御的上限とする（拒否のみ。クランプはビットストリームを壊す）。
///
/// Annex B バイトストリーム上の NAL がこれを超える理論ケースは、本上限では拒否される（実運用では稀）。
const MAX_PARAMETER_SET_COPY_BYTES: usize = u16::MAX as usize;

/// エンコード出力 1 フレーム分を `Vec` にコピーするときの防御的上限（バイト）。
///
/// `MAX_PARAMETER_SET_COPY_BYTES`（パラメータセット用）とは別。`CMBlockBufferGetDataLength` が異常に大きい場合の OOM を防ぐ。
/// 値は保守的に大きめ（4K・高ビットレート等を想定）。超過時はログして当該フレームを破棄する。
const MAX_ENCODED_BLOCK_COPY_BYTES: usize = 256 * 1024 * 1024;

/// `slice::from_raw_parts` の前提（長さ 0 でも非 NULL ポインタ、長さ正では NULL 禁止）を満たすためのヘルパー
fn vec_u8_from_raw_parts_safe(
    ptr: *const u8,
    len: usize,
    context: &'static str,
) -> Option<Vec<u8>> {
    if len > MAX_PARAMETER_SET_COPY_BYTES {
        log::error!(
            "{context}: parameter set length {len} exceeds defensive maximum {max}",
            max = MAX_PARAMETER_SET_COPY_BYTES
        );
        return None;
    }
    if len == 0 {
        return Some(Vec::new());
    }
    if ptr.is_null() {
        log::error!("{context}: null pointer with non-zero length");
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len).to_vec() })
}

fn is_keyframe(sample_buffer: sys::CMSampleBufferRef) -> bool {
    unsafe {
        let attachments = sys::CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 1);
        if attachments.is_null() {
            return false;
        }

        if sys::CFArrayGetCount(attachments) == 0 {
            return false;
        }

        let attachment = sys::CFArrayGetValueAtIndex(attachments, 0);
        if attachment.is_null() {
            return false;
        }
        // 添付が CFDictionary でない場合は NotSync キーを解釈できない
        if sys::CFGetTypeID(attachment as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
            return false;
        }

        let not_sync = sys::CFDictionaryGetValue(
            attachment as *mut _,
            sys::kCMSampleAttachmentKey_NotSync as *const c_void,
        );
        not_sync != sys::kCFBooleanTrue as *const c_void
    }
}

#[cfg(test)]
mod tests {
    //! `Encoder::reconfigure` の内部状態 (`next_input_pts`) を直接確認するためのテスト。
    //! `tests/test_encoder.rs` からは到達できない private フィールド検証だけをここに置く。
    //! 通常系の検証は `tests/test_encoder.rs` 側にある。

    use super::*;

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
            average_bitrate: None,
            expected_frame_rate: Some(60),
        })?;

        // 物理時間 2002/30000 ≒ 0.0667 秒に対応する新 timescale 60 での PTS は 4.004。
        // PTS の単調性を保つため切り上げ (div_ceil) を採用しているので 5 となる。
        assert_eq!(encoder.config().fps_numerator, 60);
        assert_eq!(encoder.config().fps_denominator, 1);
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
            expected_frame_rate: None,
        })?;

        assert_eq!(encoder.next_input_pts, before);
        Ok(())
    }

    #[test]
    fn reconfigure_overflows_when_rescaled_pts_exceeds_i64_max() -> Result<(), Error> {
        // i64 の上限を意図的に超える条件で再スケールし、`Error::LimitExceeded` が返ることを確認する。
        // 初期 timescale を 1 にしておくと encode を 1 回するごとに next_input_pts が +1 進む。
        // ここでは VTCompressionSession を起動するコストを避けるため、構築後に next_input_pts を
        // 直接 i64::MAX に書き込んでから reconfigure を呼ぶ (private フィールドなのでこのモジュールから可能)。
        let config = base_encoder_config();
        let mut encoder = Encoder::new(config, noop_handler())?;

        encoder.next_input_pts = i64::MAX;

        // new_timescale > old_timescale となる組合せで `i64::MAX * 2 / 1` が i64 範囲を超える。
        let err = encoder
            .reconfigure(ReconfigureParams {
                average_bitrate: None,
                expected_frame_rate: Some(2),
            })
            .expect_err("rescale should overflow");
        assert!(matches!(
            err,
            Error::LimitExceeded {
                reason: "rescaled presentation timestamp overflow",
            }
        ));
        // 失敗時には config も next_input_pts も変更されない
        assert_eq!(encoder.config().fps_numerator, 1);
        assert_eq!(encoder.next_input_pts, i64::MAX);
        Ok(())
    }
}
