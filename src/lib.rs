//! [Hisui] 用の [Video Toolbox] エンコーダーおよびデコーダー
//!
//! [Hisui]: https://github.com/shiguredo/hisui
//! [Video Toolbox]: https://developer.apple.com/documentation/videotoolbox/
#![warn(missing_docs)]

// macOS 以外ではビルドを許可しない (cargo doc 時は除外)
#[cfg(all(not(target_os = "macos"), not(doc)))]
compile_error!("this crate only supports macOS");

use std::{
    collections::HashMap,
    ffi::{c_int, c_void},
    marker::PhantomData,
    mem::MaybeUninit,
    num::NonZeroU32,
    time::Duration,
};

use sys::VTCompressionSessionCreate;

mod sys;

/// エラー
#[derive(Debug)]
pub enum Error {
    /// Video Toolbox API のエラー
    VideoToolbox {
        /// ステータスコード
        status: i32,
        /// 関数名
        function: &'static str,
    },
    /// ピクセルフォーマットの不一致
    PixelFormatMismatch {
        /// 期待するピクセルフォーマット
        expected: PixelFormat,
        /// 実際のピクセルフォーマット
        actual: PixelFormat,
    },
    /// フレームデータのサイズ不足
    InsufficientFrameData {
        /// プレーン名
        plane: &'static str,
        /// 期待する最小サイズ
        expected: usize,
        /// 実際のサイズ
        actual: usize,
    },
    /// コーデックが未対応
    UnsupportedCodec {
        /// コーデック名
        codec: &'static str,
    },
}

impl Error {
    fn check(status: i32, function: &'static str) -> Result<(), Self> {
        if status == 0 {
            return Ok(());
        }
        Err(Self::VideoToolbox { status, function })
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VideoToolbox { status, function } => {
                write!(
                    f,
                    "[{}] {}() failed: status={}",
                    env!("CARGO_PKG_NAME"),
                    function,
                    status
                )
            }
            Self::PixelFormatMismatch { expected, actual } => {
                write!(
                    f,
                    "pixel format mismatch: encoder expects {expected:?}, but got {actual:?}"
                )
            }
            Self::InsufficientFrameData {
                plane,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "insufficient frame data for {plane} plane: expected at least {expected} bytes, but got {actual}"
                )
            }
            Self::UnsupportedCodec { codec } => {
                write!(f, "codec {codec} is not supported on this platform")
            }
        }
    }
}

impl std::error::Error for Error {}

/// ピクセルフォーマット
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// kCVPixelFormatType_420YpCbCr8Planar (3 プレーン: Y, U, V)
    I420,
    /// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange (2 プレーン: Y, UV interleaved)
    Nv12,
}

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

// パラメータセット (VPS, SPS, PPS) のタプル型
type ParameterSets = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>);

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

/// H.264 / H.265 エンコーダー
#[derive(Debug)]
pub struct Encoder {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    next_output_pts: i64,
    output_frames: HashMap<i64, EncodedFrame>, // キーは pts
    encoded_frame_rx: std::sync::mpsc::Receiver<EncodedFrame>,

    encoded_frame_tx: Box<std::sync::mpsc::Sender<EncodedFrame>>,
}

impl Encoder {
    /// エンコーダーのインスタンスを生成する
    pub fn new(config: EncoderConfig) -> Result<Self, Error> {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Box::new(tx);
        let session = unsafe { Self::create_compression_session(&config, &tx)? };

        Ok(Self {
            session,
            config,
            next_input_pts: 0,
            next_output_pts: 0,
            output_frames: HashMap::new(),
            encoded_frame_tx: tx,
            encoded_frame_rx: rx,
        })
    }

    /// 新しい設定でエンコーダーを再作成する
    ///
    /// 未出力フレームをフラッシュした後、既存のセッションを破棄して
    /// 新しい設定でセッションを再作成する。
    /// フラッシュされたフレームは `next_frame()` で取得できる。
    pub fn reconfigure(&mut self, config: EncoderConfig) -> Result<(), Error> {
        // 未出力フレームをフラッシュ
        self.finish()?;

        unsafe {
            // チャネルを再作成し、新しいセッションを先に作成する
            // 失敗時に self が不整合にならないようにする
            let (tx, rx) = std::sync::mpsc::channel();
            let tx = Box::new(tx);
            let session = Self::create_compression_session(&config, &tx)?;

            // 新しいセッションの作成に成功してから既存のセッションを破棄する
            sys::VTCompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);

            self.session = session;
            self.config = config;
            self.next_input_pts = 0;
            self.next_output_pts = 0;
            self.output_frames.clear();
            self.encoded_frame_tx = tx;
            self.encoded_frame_rx = rx;
        }

        Ok(())
    }

    /// EncoderConfig と Sender から VTCompressionSession を作成する
    unsafe fn create_compression_session(
        config: &EncoderConfig,
        tx: &std::sync::mpsc::Sender<EncodedFrame>,
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

            let status = VTCompressionSessionCreate(
                std::ptr::null_mut(),
                config.width as i32,
                config.height as i32,
                codec_fourcc,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(callback),
                (tx as *const std::sync::mpsc::Sender<EncodedFrame>)
                    .cast::<c_void>()
                    .cast_mut(),
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

            let properties_dict = cf_dictionary(&properties);
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
            let fps = cf_number_i32(config.fps_numerator.div_ceil(config.fps_denominator) as i32);

            // ビットレート (指定時のみ設定)
            if let Some(bitrate) = config.average_bitrate {
                let target_bitrate = cf_number_i64(bitrate as i64);
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
                let interval_value = cf_number_i32(interval.get() as i32);
                properties.push((
                    sys::kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    interval_value.0,
                ));
                cf_objects.push(interval_value);
            }

            // キーフレーム間隔（秒数）
            if let Some(duration) = config.max_key_frame_interval_duration {
                let duration_value = cf_number_f64(duration.as_secs_f64());
                properties.push((
                    sys::kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
                    duration_value.0,
                ));
                cf_objects.push(duration_value);
            }

            // フレーム遅延制限
            if let Some(delay_count) = config.max_frame_delay_count {
                let delay_value = cf_number_i32(delay_count.get() as i32);
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

    /// フレームデータの長さが必要なサイズを満たしているか検証する
    fn validate_frame_data(
        frame: &FrameData<'_>,
        width: usize,
        height: usize,
    ) -> Result<(), Error> {
        match frame {
            FrameData::I420 { y, u, v } => {
                let y_expected = width * height;
                let uv_expected = width.div_ceil(2) * height.div_ceil(2);
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
                let y_expected = width * height;
                let uv_expected = width * height.div_ceil(2);
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

    /// 画像データをエンコードする
    ///
    /// エンコード結果は [`Encoder::next_frame()`] で取得できる
    ///
    /// なお `y` のストライドは入力フレームの幅と等しいことが前提
    ///
    /// また B フレームは扱わない前提（つまり入力フレームと出力フレームの順番が一致する）
    pub fn encode(&mut self, frame: &FrameData<'_>, options: &EncodeOptions) -> Result<(), Error> {
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
            let mut image_buffer = std::ptr::null_mut();
            let status = match frame {
                FrameData::I420 { y, u, v } => sys::CVPixelBufferCreateWithPlanarBytes(
                    std::ptr::null_mut(),
                    width,
                    height,
                    u32::from_be_bytes(*b"y420"),
                    std::ptr::null_mut(),
                    0,
                    3,
                    [
                        y.as_ptr().cast::<c_void>().cast_mut(),
                        u.as_ptr().cast::<c_void>().cast_mut(),
                        v.as_ptr().cast::<c_void>().cast_mut(),
                    ]
                    .as_mut_ptr(),
                    [width, width.div_ceil(2), width.div_ceil(2)].as_mut_ptr(),
                    [height, height.div_ceil(2), height.div_ceil(2)].as_mut_ptr(),
                    [width, width.div_ceil(2), width.div_ceil(2)].as_mut_ptr(),
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut image_buffer,
                ),
                FrameData::Nv12 { y, uv } => sys::CVPixelBufferCreateWithPlanarBytes(
                    std::ptr::null_mut(),
                    width,
                    height,
                    sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                    std::ptr::null_mut(),
                    0,
                    2,
                    [
                        y.as_ptr().cast::<c_void>().cast_mut(),
                        uv.as_ptr().cast::<c_void>().cast_mut(),
                    ]
                    .as_mut_ptr(),
                    [width, width].as_mut_ptr(),
                    [height, height.div_ceil(2)].as_mut_ptr(),
                    [width, width].as_mut_ptr(),
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut image_buffer,
                ),
            };
            Error::check(status, "CVPixelBufferCreateWithPlanarBytes")?;

            let image_buffer = CfPtrMut(image_buffer);

            let frame_properties = if options.force_key_frame {
                cf_dictionary(&[(
                    sys::kVTEncodeFrameOptionKey_ForceKeyFrame,
                    sys::kCFBooleanTrue as *const c_void,
                )])
            } else {
                std::ptr::null()
            };
            let _frame_properties_guard = if !frame_properties.is_null() {
                Some(CfPtr(frame_properties.cast::<c_void>()))
            } else {
                None
            };

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            Error::check(status, "VTCompressionSessionEncodeFrame")?;

            self.next_input_pts += self.config.fps_denominator as i64;

            Ok(())
        }
    }

    /// CVPixelBuffer を直接エンコードする（ゼロコピー）
    ///
    /// video-device-rs の `PixelBuffer::as_ptr()` で取得した CVPixelBuffer ポインタを渡す。
    /// 内部で CFRetain するため、呼び出し元はこの関数の後にポインタ元を drop してよい。
    ///
    /// エンコード結果は [`Encoder::next_frame()`] で取得できる
    ///
    /// # Safety
    ///
    /// `pixel_buffer_ptr` は有効な CVPixelBuffer ポインタでなければならない。
    pub unsafe fn encode_pixel_buffer(
        &mut self,
        pixel_buffer_ptr: *mut c_void,
        options: &EncodeOptions,
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
                )])
            } else {
                std::ptr::null()
            };
            let _frame_properties_guard = if !frame_properties.is_null() {
                Some(CfPtr(frame_properties.cast::<c_void>()))
            } else {
                None
            };

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            Error::check(status, "VTCompressionSessionEncodeFrame")?;

            self.next_input_pts += self.config.fps_denominator as i64;

            Ok(())
        }
    }

    /// これ以上データが来ないことをエンコーダーに伝える
    ///
    /// 残りのエンコード結果は [`Encoder::next_frame()`] で取得できる
    pub fn finish(&mut self) -> Result<(), Error> {
        unsafe {
            let status = sys::VTCompressionSessionCompleteFrames(self.session, sys::kCMTimeInvalid);
            Error::check(status, "VTCompressionSessionCompleteFrames")?;
        }
        Ok(())
    }

    /// エンコード済みのフレームを取り出す
    ///
    /// PTS 順に出力するため HashMap でバッファリングしている。
    /// `allow_frame_reordering: false` を前提としており、
    /// B フレームによる PTS の飛びには対応していない。
    pub fn next_frame(&mut self) -> Option<EncodedFrame> {
        let Ok(frame) = self.encoded_frame_rx.try_recv() else {
            return None;
        };
        self.output_frames.insert(frame.pts, frame);
        self.output_frames
            .remove(&self.next_output_pts)
            .inspect(|_| {
                self.next_output_pts += self.config.fps_denominator as i64;
            })
    }

    unsafe extern "C" fn output_callback_h264(
        output_callback_ref_con: *mut c_void,
        _source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
                sample_buffer,
                status,
                "output_callback_h264",
                Self::extract_h264_params,
            );
        }
    }

    unsafe extern "C" fn output_callback_h265(
        output_callback_ref_con: *mut c_void,
        _source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
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
        sample_buffer: sys::CMSampleBufferRef,
        status: i32,
        callback_name: &'static str,
        extract_params: unsafe fn(sys::CMVideoFormatDescriptionRef) -> Option<ParameterSets>,
    ) {
        if let Err(e) = Error::check(status, callback_name) {
            log::error!("{e}");
            return;
        }

        unsafe {
            let data_buffer = sys::CMSampleBufferGetDataBuffer(sample_buffer);
            let mut data_pointer = std::ptr::null_mut();
            let mut data_pointer_len = 0;
            let status = sys::CMBlockBufferGetDataPointer(
                data_buffer,
                0,
                &mut data_pointer_len,
                std::ptr::null_mut(),
                &mut data_pointer,
            );
            if let Err(e) = Error::check(status, "CMBlockBufferGetDataPointer") {
                log::error!("{e}");
                return;
            }

            let pts = sys::CMSampleBufferGetPresentationTimeStamp(sample_buffer);
            let description = sys::CMSampleBufferGetFormatDescription(sample_buffer);
            let data =
                std::slice::from_raw_parts(data_pointer as *const u8, data_pointer_len).to_vec();
            let keyframe = is_keyframe(sample_buffer);

            let (vps_list, sps_list, pps_list) = if keyframe {
                match extract_params(description) {
                    Some(params) => params,
                    None => return,
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
                pts: pts.value,
            };

            // 呼び出しもとスレッドに結果を伝える
            // (Sender は Send を実装しているので、複数スレッドで参照を共有しても問題ない)
            let tx = &*(output_callback_ref_con as *mut std::sync::mpsc::Sender<EncodedFrame>);
            let _ = tx.send(frame);
        }
    }

    /// H.264 のパラメータセット (SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル (H.264 では vps_list は空)
    unsafe fn extract_h264_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Option<ParameterSets> {
        unsafe {
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

            Some((
                Vec::new(), // H.264 には VPS は存在しない
                vec![std::slice::from_raw_parts(sps_ptr, sps_size).to_vec()],
                vec![std::slice::from_raw_parts(pps_ptr, pps_size).to_vec()],
            ))
        }
    }

    /// H.265 のパラメータセット (VPS, SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル
    unsafe fn extract_h265_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Option<ParameterSets> {
        unsafe {
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

            Some((
                vec![std::slice::from_raw_parts(vps_ptr, vps_size).to_vec()],
                vec![std::slice::from_raw_parts(sps_ptr, sps_size).to_vec()],
                vec![std::slice::from_raw_parts(pps_ptr, pps_size).to_vec()],
            ))
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            sys::VTCompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);
        }
    }
}

// SAFETY: VTCompressionSession は内部でスレッドセーフに管理されており、
// Apple のドキュメントでもセッションの操作は異なるスレッドから呼び出し可能とされている。
// コールバックは別スレッドから呼ばれるが、mpsc::Sender 経由でデータを渡すため安全。
unsafe impl Send for Encoder {}

/// エンコードされた映像フレーム (AVCC 形式)
#[derive(Debug)]
pub struct EncodedFrame {
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

    pts: i64,
}

fn is_keyframe(sample_buffer: sys::CMSampleBufferRef) -> bool {
    unsafe {
        let attachments = sys::CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 1);
        if attachments.is_null() {
            return false;
        }

        let attachment = sys::CFArrayGetValueAtIndex(attachments, 0);
        if attachment.is_null() {
            return false;
        }

        let not_sync = sys::CFDictionaryGetValue(
            attachment as *mut _,
            sys::kCMSampleAttachmentKey_NotSync as *const c_void,
        );
        not_sync != sys::kCFBooleanTrue as *const c_void
    }
}

/// デコーダーのコーデック設定
pub enum DecoderCodec<'a> {
    /// CMVideoFormatDescriptionCreateFromH264ParameterSets
    H264 {
        /// Sequence Parameter Set
        sps: &'a [u8],
        /// Picture Parameter Set
        pps: &'a [u8],
        /// NAL ユニット長フィールドのバイト数
        nalu_len_bytes: u32,
    },
    /// CMVideoFormatDescriptionCreateFromHEVCParameterSets
    Hevc {
        /// Video Parameter Set
        vps: &'a [u8],
        /// Sequence Parameter Set
        sps: &'a [u8],
        /// Picture Parameter Set
        pps: &'a [u8],
        /// NAL ユニット長フィールドのバイト数
        nalu_len_bytes: u32,
    },
    /// CMVideoFormatDescriptionCreate (codec_type: vp09)
    Vp9 {
        /// 映像の幅
        width: u32,
        /// 映像の高さ
        height: u32,
    },
    /// CMVideoFormatDescriptionCreate (codec_type: av01)
    Av1 {
        /// 映像の幅
        width: u32,
        /// 映像の高さ
        height: u32,
    },
}

/// デコーダーの設定
pub struct DecoderConfig<'a> {
    /// コーデック固有の設定
    pub codec: DecoderCodec<'a>,
    /// 出力ピクセルフォーマット
    pub pixel_format: PixelFormat,
}

/// H.264 / H.265 / VP9 / AV1 デコーダー
#[derive(Debug)]
pub struct Decoder {
    description: sys::CMVideoFormatDescriptionRef,
    session: sys::VTDecompressionSessionRef,
    pixel_format: PixelFormat,
}

impl Decoder {
    /// デコーダーのインスタンスを生成する
    pub fn new(config: DecoderConfig<'_>) -> Result<Self, Error> {
        unsafe {
            let description = Self::create_format_description(&config.codec)
                .map_err(|e| Self::wrap_unsupported_codec_error(&config.codec, e))?;
            let session = Self::create_decompression_session(description, config.pixel_format)
                .map_err(|e| {
                    sys::CFRelease(description as *const c_void);
                    Self::wrap_unsupported_codec_error(&config.codec, e)
                })?;

            Ok(Self {
                description,
                session,
                pixel_format: config.pixel_format,
            })
        }
    }

    /// VP9/AV1 コーデックの場合、エラーを UnsupportedCodec に変換する
    fn wrap_unsupported_codec_error(codec: &DecoderCodec<'_>, error: Error) -> Error {
        match codec {
            DecoderCodec::Vp9 { .. } => Error::UnsupportedCodec { codec: "VP9" },
            DecoderCodec::Av1 { .. } => Error::UnsupportedCodec { codec: "AV1" },
            _ => error,
        }
    }

    /// 新しいパラメータセットでデコーダーのフォーマットを更新する
    ///
    /// Video Toolbox の `VTDecompressionSessionCanAcceptFormatDescription()` で
    /// 現在のセッションが新しい FormatDescription を受け入れ可能か判定し、
    /// 受け入れ不可能な場合はセッションを再作成する。
    /// 受け入れ可能な場合は FormatDescription のみ更新する。
    pub fn update_format(&mut self, codec: DecoderCodec<'_>) -> Result<(), Error> {
        unsafe {
            let new_description = Self::create_format_description(&codec)?;

            let can_accept = sys::VTDecompressionSessionCanAcceptFormatDescription(
                self.session,
                new_description,
            );

            if can_accept != 0 {
                // 受け入れ可能: description のみ差し替え
                sys::CFRelease(self.description as *const c_void);
                self.description = new_description;
            } else {
                // 受け入れ不可能: セッションを再作成
                // 新しいセッションを先に作成し、失敗時に self が不整合にならないようにする
                let new_session =
                    match Self::create_decompression_session(new_description, self.pixel_format) {
                        Ok(session) => session,
                        Err(e) => {
                            sys::CFRelease(new_description as *const c_void);
                            return Err(e);
                        }
                    };

                sys::VTDecompressionSessionInvalidate(self.session);
                sys::CFRelease(self.session as *const c_void);
                sys::CFRelease(self.description as *const c_void);
                self.description = new_description;
                self.session = new_session;
            }

            Ok(())
        }
    }

    /// DecoderCodec から CMVideoFormatDescription を作成する
    unsafe fn create_format_description(
        codec: &DecoderCodec<'_>,
    ) -> Result<sys::CMVideoFormatDescriptionRef, Error> {
        unsafe {
            let mut description: sys::CMVideoFormatDescriptionRef = std::ptr::null_mut();

            match codec {
                DecoderCodec::H264 {
                    sps,
                    pps,
                    nalu_len_bytes,
                } => {
                    let status = sys::CMVideoFormatDescriptionCreateFromH264ParameterSets(
                        std::ptr::null_mut(),
                        2,
                        [sps.as_ptr(), pps.as_ptr()].as_ptr(),
                        [sps.len(), pps.len()].as_ptr(),
                        *nalu_len_bytes as c_int,
                        &mut description,
                    );
                    Error::check(
                        status,
                        "CMVideoFormatDescriptionCreateFromH264ParameterSets",
                    )?;
                }
                DecoderCodec::Hevc {
                    vps,
                    sps,
                    pps,
                    nalu_len_bytes,
                } => {
                    let status = sys::CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                        std::ptr::null_mut(),
                        3,
                        [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()].as_ptr(),
                        [vps.len(), sps.len(), pps.len()].as_ptr(),
                        *nalu_len_bytes as c_int,
                        std::ptr::null_mut(),
                        &mut description,
                    );
                    Error::check(
                        status,
                        "CMVideoFormatDescriptionCreateFromHEVCParameterSets",
                    )?;
                }
                DecoderCodec::Vp9 { width, height } => {
                    let status = sys::CMVideoFormatDescriptionCreate(
                        std::ptr::null_mut(),
                        u32::from_be_bytes(*b"vp09"),
                        *width as i32,
                        *height as i32,
                        std::ptr::null_mut(),
                        &mut description,
                    );
                    Error::check(status, "CMVideoFormatDescriptionCreate")?;
                }
                DecoderCodec::Av1 { width, height } => {
                    let status = sys::CMVideoFormatDescriptionCreate(
                        std::ptr::null_mut(),
                        u32::from_be_bytes(*b"av01"),
                        *width as i32,
                        *height as i32,
                        std::ptr::null_mut(),
                        &mut description,
                    );
                    Error::check(status, "CMVideoFormatDescriptionCreate")?;
                }
            }

            Ok(description)
        }
    }

    /// CMVideoFormatDescription と PixelFormat から VTDecompressionSession を作成する
    unsafe fn create_decompression_session(
        description: sys::CMVideoFormatDescriptionRef,
        pixel_format: PixelFormat,
    ) -> Result<sys::VTDecompressionSessionRef, Error> {
        unsafe {
            let mut session: sys::VTDecompressionSessionRef = std::ptr::null_mut();
            let mut callback =
                MaybeUninit::<sys::VTDecompressionOutputCallbackRecord>::zeroed().assume_init();
            callback.decompressionOutputCallback = Some(Self::output_callback);

            let cv_pixel_format = match pixel_format {
                PixelFormat::I420 => sys::kCVPixelFormatType_420YpCbCr8Planar,
                PixelFormat::Nv12 => sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            };
            let pf = cf_number_i32(cv_pixel_format as i32);
            let dest_attrs = cf_dictionary(&[(sys::kCVPixelBufferPixelFormatTypeKey, pf.0)]);
            let _dest_attrs_guard = CfPtr(dest_attrs.cast::<c_void>());
            let status = sys::VTDecompressionSessionCreate(
                std::ptr::null_mut(),
                description,
                std::ptr::null_mut(),
                dest_attrs,
                &callback,
                &mut session,
            );
            Error::check(status, "VTDecompressionSessionCreate")?;

            Ok(session)
        }
    }

    /// 圧縮された映像フレームをデコードする
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedFrame<'_>>, Error> {
        unsafe {
            let mut block_buffer = std::ptr::null_mut();
            let status = sys::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null_mut(),
                data.as_ptr().cast_mut().cast(),
                data.len(),
                sys::kCFAllocatorNull, // data の自動解放を Video Toolbox 側で行わないようにする
                std::ptr::null(),
                0,
                data.len(),
                0,
                &mut block_buffer,
            );
            Error::check(status, "CMBlockBufferCreateWithMemoryBlock")?;
            let block_buffer = CfPtrMut(block_buffer);

            let mut sample_buffer = std::ptr::null_mut();
            let status = sys::CMSampleBufferCreateReady(
                std::ptr::null_mut(),
                block_buffer.0,
                self.description,
                1,
                0,
                [].as_ptr(),
                0,
                [].as_ptr(),
                &mut sample_buffer,
            );
            Error::check(status, "CMSampleBufferCreateReady")?;
            let sample_buffer = CfPtrMut(sample_buffer);

            let decode_flags = 0;
            let mut info_flags = 0;
            let mut image_buffer: sys::CVImageBufferRef = std::ptr::null_mut();
            let status = sys::VTDecompressionSessionDecodeFrame(
                self.session,
                sample_buffer.0,
                decode_flags,
                ((&mut image_buffer) as *mut sys::CVImageBufferRef).cast(),
                &mut info_flags,
            );
            Error::check(status, "VTDecompressionSessionDecodeFrame")?;

            if image_buffer.is_null() {
                return Ok(None);
            }

            let image_buffer = CfPtrMut(image_buffer);
            let flags_readonly = 1;
            let status = sys::CVPixelBufferLockBaseAddress(image_buffer.0, flags_readonly);
            Error::check(status, "CVPixelBufferLockBaseAddress")?;

            let frame = match self.pixel_format {
                PixelFormat::I420 => DecodedFrame::I420(I420Frame {
                    inner: image_buffer,
                    _lifetime: PhantomData,
                }),
                PixelFormat::Nv12 => DecodedFrame::Nv12(Nv12Frame {
                    inner: image_buffer,
                    _lifetime: PhantomData,
                }),
            };
            Ok(Some(frame))
        }
    }

    // [NOTE] このコールバック関数は VTDecompressionSessionDecodeFrame() の処理中に呼び出される
    //        (指定したフラグによって挙動は変わるがデフォルトでは）
    unsafe extern "C" fn output_callback(
        _decompression_output_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTDecodeInfoFlags,
        image_buffer: sys::CVImageBufferRef,
        _presentation_time_stamp: sys::CMTime,
        _presentation_duration: sys::CMTime,
    ) {
        if let Err(e) = Error::check(status, "output_callback") {
            log::error!("{e}");
            return;
        }

        let output = source_frame_ref_con.cast();
        unsafe {
            *output = sys::CFRetain(image_buffer.cast());
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            sys::VTDecompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);
            sys::CFRelease(self.description as *const c_void);
        }
    }
}

// SAFETY: VTDecompressionSession は内部でスレッドセーフに管理されており、
// Apple のドキュメントでもセッションの操作は異なるスレッドから呼び出し可能とされている。
// デコードコールバックは同期的に呼ばれるため、並行アクセスの問題は発生しない。
unsafe impl Send for Decoder {}

/// デコードされた映像フレーム
pub enum DecodedFrame<'a> {
    /// I420 形式
    I420(I420Frame<'a>),
    /// NV12 形式
    Nv12(Nv12Frame<'a>),
}

/// I420 形式のデコード済みフレーム (3 プレーン: Y, U, V)
#[derive(Debug)]
pub struct I420Frame<'a> {
    inner: CfPtrMut<sys::__CVBuffer>,

    // inner の中には Video Toolbox が返した一時的なデータへの参照も含まれているので、
    // このライフタイムで利用側での使用範囲を制限する。
    _lifetime: PhantomData<&'a ()>,
}

impl I420Frame<'_> {
    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, 0) as *const u8,
                self.height() * self.y_stride(),
            )
        }
    }

    /// フレームの U 成分のデータを返す
    pub fn u_plane(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, 1) as *const u8,
                self.height().div_ceil(2) * self.u_stride(),
            )
        }
    }

    /// フレームの V 成分のデータを返す
    pub fn v_plane(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, 2) as *const u8,
                self.height().div_ceil(2) * self.v_stride(),
            )
        }
    }

    /// フレームの Y 成分のストライドを返す
    pub fn y_stride(&self) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, 0) }
    }

    /// フレームの U 成分のストライドを返す
    pub fn u_stride(&self) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, 1) }
    }

    /// フレームの V 成分のストライドを返す
    pub fn v_stride(&self) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, 2) }
    }

    /// フレームの幅を返す
    pub fn width(&self) -> usize {
        unsafe { sys::CVPixelBufferGetWidth(self.inner.0) }
    }

    /// フレームの高さを返す
    pub fn height(&self) -> usize {
        unsafe { sys::CVPixelBufferGetHeight(self.inner.0) }
    }
}

impl Drop for I420Frame<'_> {
    fn drop(&mut self) {
        unsafe {
            let flags_readonly = 1;
            sys::CVPixelBufferUnlockBaseAddress(self.inner.0, flags_readonly);
        }
    }
}

/// NV12 形式のデコード済みフレーム (2 プレーン: Y, UV interleaved)
#[derive(Debug)]
pub struct Nv12Frame<'a> {
    inner: CfPtrMut<sys::__CVBuffer>,

    // inner の中には Video Toolbox が返した一時的なデータへの参照も含まれているので、
    // このライフタイムで利用側での使用範囲を制限する。
    _lifetime: PhantomData<&'a ()>,
}

impl Nv12Frame<'_> {
    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, 0) as *const u8,
                self.height() * self.y_stride(),
            )
        }
    }

    /// フレームの UV インターリーブデータを返す
    pub fn uv_plane(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, 1) as *const u8,
                self.height().div_ceil(2) * self.uv_stride(),
            )
        }
    }

    /// フレームの Y 成分のストライドを返す
    pub fn y_stride(&self) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, 0) }
    }

    /// フレームの UV インターリーブのストライドを返す
    pub fn uv_stride(&self) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, 1) }
    }

    /// フレームの幅を返す
    pub fn width(&self) -> usize {
        unsafe { sys::CVPixelBufferGetWidth(self.inner.0) }
    }

    /// フレームの高さを返す
    pub fn height(&self) -> usize {
        unsafe { sys::CVPixelBufferGetHeight(self.inner.0) }
    }
}

impl Drop for Nv12Frame<'_> {
    fn drop(&mut self) {
        unsafe {
            let flags_readonly = 1;
            sys::CVPixelBufferUnlockBaseAddress(self.inner.0, flags_readonly);
        }
    }
}

// ドロップ時に確実に sys::CFRelease() を呼び出すようにするためのラッパー
#[derive(Debug)]
struct CfPtrMut<T>(*mut T);

impl<T> Drop for CfPtrMut<T> {
    fn drop(&mut self) {
        unsafe { sys::CFRelease(self.0.cast()) }
    }
}

#[derive(Debug)]
struct CfPtr<T>(*const T);

impl<T> Drop for CfPtr<T> {
    fn drop(&mut self) {
        unsafe { sys::CFRelease(self.0.cast()) }
    }
}

fn cf_dictionary(kvs: &[(sys::CFStringRef, *const c_void)]) -> sys::CFDictionaryRef {
    let mut keys = kvs.iter().map(|(k, _)| k.cast()).collect::<Vec<_>>();
    let mut values = kvs.iter().map(|(_, v)| *v).collect::<Vec<_>>();
    unsafe {
        sys::CFDictionaryCreate(
            std::ptr::null_mut(),
            keys.as_mut_ptr(),
            values.as_mut_ptr(),
            kvs.len() as sys::CFIndex,
            &sys::kCFTypeDictionaryKeyCallBacks,
            &sys::kCFTypeDictionaryValueCallBacks,
        )
    }
}

fn cf_number_i32(n: i32) -> CfPtr<c_void> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberSInt32Type as sys::CFNumberType,
            ((&n) as *const i32).cast(),
        )
    };
    CfPtr(ptr.cast())
}

fn cf_number_i64(n: i64) -> CfPtr<c_void> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberSInt64Type as sys::CFNumberType,
            ((&n) as *const i64).cast(),
        )
    };
    CfPtr(ptr.cast())
}

fn cf_number_f64(n: f64) -> CfPtr<c_void> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberFloat64Type as sys::CFNumberType,
            ((&n) as *const f64).cast(),
        )
    };
    CfPtr(ptr.cast())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: u32 = 960;
    const HEIGHT: u32 = 480;
    const SIZE: usize = WIDTH as usize * HEIGHT as usize;

    #[test]
    fn h264_decoder() -> Result<(), Error> {
        let sps = [
            103, 100, 0, 30, 172, 217, 64, 160, 61, 176, 17, 0, 0, 3, 0, 1, 0, 0, 3, 0, 50, 15, 22,
            45, 150,
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
            102, 163, 229, 208, 146, 229, 251, 16, 96, 250, 208, 0, 0, 3, 0, 0, 3, 0, 0, 16, 15,
            210, 222, 245, 204, 98, 91, 229, 32, 0, 0, 9, 216, 2, 56, 13, 16, 118, 133, 116, 69,
            196, 32, 71, 6, 120, 150, 16, 161, 210, 50, 128, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0,
            0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 37, 225,
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
            40, 1, 175, 29, 16, 90, 181, 140, 90, 213, 247, 1, 91, 255, 242, 78, 254, 199, 0, 31,
            209, 50, 148, 21, 162, 38, 146, 0, 0, 3, 1, 203, 169, 113, 202, 5, 24, 129, 39, 128, 0,
            0, 3, 0, 7, 204, 147, 13, 148, 32, 0, 0, 3, 0, 0, 3, 0, 12, 24, 135, 0, 0, 3, 0, 0, 3,
            0, 0, 3, 0, 28, 240, 0, 0, 3, 0, 0, 3, 0, 0, 3, 0, 8, 104, 0, 0, 3, 0, 0, 3, 0, 0, 3,
            0, 104, 192, 0, 0, 3, 0, 0, 3, 0, 0, 3, 1, 223, 0, 0, 3, 0, 9, 248,
        ];
        let mut data = Vec::new();
        data.extend_from_slice(&(nal_unit.len() as u32).to_be_bytes());
        data.extend_from_slice(&nal_unit);
        decoder.decode(&data)?;

        Ok(())
    }

    #[test]
    fn init_h264_encoder() {
        // OK
        let config = encoder_config(false);
        assert!(Encoder::new(config).is_ok());

        // NG
        let mut config = encoder_config(false);
        config.width = 0;
        assert!(Encoder::new(config).is_err());
    }

    #[test]
    fn init_h265_encoder() {
        // OK
        let config = encoder_config(true);
        assert!(Encoder::new(config).is_ok());

        // NG
        let mut config = encoder_config(true);
        config.width = 0;
        assert!(Encoder::new(config).is_err());
    }

    #[test]
    fn encode_h264_black() {
        let config = encoder_config(false);
        let mut encoder = Encoder::new(config).expect("create encoder error");
        let mut count = 0;

        // [NOTE]: encode(&[0; SIZE], ..) の様に変数を経由せずに指定するとエラーになる
        let y = [0; SIZE];
        let u = [0; SIZE / 4];
        let v = [0; SIZE / 4];
        encoder
            .encode(
                &FrameData::I420 {
                    y: &y,
                    u: &u,
                    v: &v,
                },
                &EncodeOptions::default(),
            )
            .expect("encode error");

        while encoder.next_frame().is_some() {
            count += 1;
        }

        encoder.finish().expect("finish error");
        while encoder.next_frame().is_some() {
            count += 1;
        }

        assert_eq!(count, 1);
    }

    #[test]
    fn encode_h265_black() {
        let config = encoder_config(true);
        let mut encoder = Encoder::new(config).expect("create encoder error");
        let mut count = 0;

        // [NOTE]: encode(&[0; SIZE], ..) の様に変数を経由せずに指定するとエラーになる
        let y = [0; SIZE];
        let u = [0; SIZE / 4];
        let v = [0; SIZE / 4];
        encoder
            .encode(
                &FrameData::I420 {
                    y: &y,
                    u: &u,
                    v: &v,
                },
                &EncodeOptions::default(),
            )
            .expect("encode error");

        while encoder.next_frame().is_some() {
            count += 1;
        }

        encoder.finish().expect("finish error");
        while encoder.next_frame().is_some() {
            count += 1;
        }

        assert_eq!(count, 1);
    }

    #[test]
    fn init_vp9_decoder() {
        let result = Decoder::new(DecoderConfig {
            codec: DecoderCodec::Vp9 {
                width: WIDTH,
                height: HEIGHT,
            },
            pixel_format: PixelFormat::I420,
        });
        if let Err(Error::UnsupportedCodec { codec }) = &result {
            eprintln!("VP9 decoder not supported on this platform: {codec}");
            return;
        }
        result.expect("failed to create VP9 decoder");
    }

    #[test]
    fn init_av1_decoder() {
        let result = Decoder::new(DecoderConfig {
            codec: DecoderCodec::Av1 {
                width: WIDTH,
                height: HEIGHT,
            },
            pixel_format: PixelFormat::I420,
        });
        if let Err(Error::UnsupportedCodec { codec }) = &result {
            eprintln!("AV1 decoder not supported on this platform: {codec}");
            return;
        }
        result.expect("failed to create AV1 decoder");
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
                    let u =
                        (-0.148 * rf - 0.291 * gf + 0.439 * bf + 128.0).clamp(16.0, 240.0) as u8;
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
        use shiguredo_libvpx::{
            CodecConfig as VpxCodecConfig, EncodeOptions as VpxEncodeOptions,
            Encoder as VpxEncoder, EncoderConfig as VpxEncoderConfig,
            EncodingDeadline as VpxEncodingDeadline, ImageData as VpxImageData,
            ImageFormat as VpxImageFormat, RateControlMode as VpxRateControlMode,
            Vp9Config as VpxVp9Config,
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
        let mut vpx_encoder = VpxEncoder::new(vpx_config).expect("failed to create VP9 encoder");

        let mut encoded_frames: Vec<Vec<u8>> = Vec::new();
        for i in 0..num_frames {
            vpx_encoder
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
                .expect("failed to encode VP9 frame");
            while let Some(frame) = vpx_encoder.next_frame() {
                encoded_frames.push(frame.data().to_vec());
            }
        }
        vpx_encoder.finish().expect("failed to finish VP9 encoder");
        while let Some(frame) = vpx_encoder.next_frame() {
            encoded_frames.push(frame.data().to_vec());
        }

        assert!(!encoded_frames.is_empty(), "VP9 encoder produced no frames");

        // Video Toolbox VP9 デコーダーを作成
        let mut decoder = match Decoder::new(DecoderConfig {
            codec: DecoderCodec::Vp9 { width, height },
            pixel_format: PixelFormat::I420,
        }) {
            Ok(decoder) => decoder,
            Err(Error::UnsupportedCodec { codec }) => {
                eprintln!("VP9 decoder not supported on this platform: {codec}");
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        // 各フレームをデコードして PSNR を検証
        let min_psnr_db = 25.0;
        for (i, encoded_data) in encoded_frames.iter().enumerate() {
            let decoded = decoder
                .decode(encoded_data)?
                .unwrap_or_else(|| panic!("frame {i}: decode returned None"));

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
                    panic!("frame {i}: expected I420 but got NV12");
                }
            }
        }

        Ok(())
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
}
