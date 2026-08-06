//! FFI セッション生成とプロパティ構築 (CF オブジェクトの変換ヘルパーを含む)

use std::ffi::c_void;

use crate::{
    encoder::{
        Encoder,
        config::{
            CodecConfig, DataRateLimit, EncoderConfig, H264EncoderConfig, H264EntropyMode,
            H264Profile, HevcEncoderConfig, HevcProfile,
        },
        handler::EncodeHandler,
    },
    error::Error,
    sys::{self, VTCompressionSessionCreate},
    types::{
        CfPtr, CfPtrMut, cf_array, cf_dictionary, cf_number_f64, cf_number_i32, cf_number_i64,
    },
};

/// `data_rate_limits` を CFArray に変換して properties に追加する
///
/// kVTCompressionPropertyKey_DataRateLimits は「bytes, seconds を交互に並べた偶数個の
/// CFNumber の CFArray」と規定されている (VTCompressionProperties.h)。
/// CFArray は各要素を retain するため、要素の CFNumber は本関数内で drop してよい。
pub(super) fn push_data_rate_limits_property(
    properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
    cf_objects: &mut Vec<CfPtr<c_void>>,
    limits: &[DataRateLimit],
) -> Result<(), Error> {
    let mut elements: Vec<CfPtr<c_void>> = Vec::new();
    let mut raw: Vec<*const c_void> = Vec::new();
    for limit in limits {
        let bytes = cf_number_i64(limit.bytes as i64)?;
        let seconds = cf_number_f64(limit.window.as_secs_f64())?;
        raw.push(bytes.0);
        raw.push(seconds.0);
        elements.push(bytes);
        elements.push(seconds);
    }
    let array = cf_array(&raw)?;
    unsafe {
        properties.push((sys::kVTCompressionPropertyKey_DataRateLimits, array.0));
    }
    cf_objects.push(array);
    Ok(())
}

/// `average_bitrate` を CFNumber 化して `kVTCompressionPropertyKey_AverageBitRate` に設定する
///
/// properties に積んだ生ポインタが辞書生成まで有効でいるためには、生成した CFNumber の
/// 所有を `cf_objects` に移して保持し続ける必要がある。`cf_objects` への push を忘れると
/// use-after-free になる (呼び出し側スコープ末尾の drop で CFRelease する)。
/// `bitrate_bps` は `validate_average_bitrate` で `i64::MAX` 以下に検証済みのため、
/// `as i64` の切り詰めは発生しない。
pub(super) fn push_bitrate_property(
    properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
    cf_objects: &mut Vec<CfPtr<c_void>>,
    bitrate_bps: u64,
) -> Result<(), Error> {
    let value = cf_number_i64(bitrate_bps as i64)?;
    unsafe {
        properties.push((sys::kVTCompressionPropertyKey_AverageBitRate, value.0));
    }
    cf_objects.push(value);
    Ok(())
}

/// 整数 fps を CFNumber 化して `kVTCompressionPropertyKey_ExpectedFrameRate` に設定する
///
/// properties に積んだ生ポインタが辞書生成まで有効でいるためには、生成した CFNumber の
/// 所有を `cf_objects` に移して保持し続ける必要がある。`cf_objects` への push を忘れると
/// use-after-free になる (呼び出し側スコープ末尾の drop で CFRelease する)。
/// `fps` は呼び出し側で `i32::MAX` 以下に検証済み (`validate_positive_i32_field`。
/// `add_common_properties` 側は検証済みの `fps_numerator` を `div_ceil` で丸めた値) のため、
/// `as i32` の切り詰めは発生しない。
pub(super) fn push_expected_frame_rate_property(
    properties: &mut Vec<(sys::CFStringRef, *const c_void)>,
    cf_objects: &mut Vec<CfPtr<c_void>>,
    fps: u32,
) -> Result<(), Error> {
    let value = cf_number_i32(fps as i32)?;
    unsafe {
        properties.push((sys::kVTCompressionPropertyKey_ExpectedFrameRate, value.0));
    }
    cf_objects.push(value);
    Ok(())
}

impl<H: EncodeHandler> Encoder<H> {
    /// EncoderConfig と完了コールバックから VTCompressionSession を作成する
    ///
    /// 成功時はセッションの所有権が呼び出し元に移り、`Encoder::drop` が解放する。
    /// 失敗時は生成済みセッションを関数内で解放してから `Err` を返す。
    pub(super) unsafe fn create_compression_session(
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

            // 生成したセッションをガードし、以降のプロパティ設定が失敗して早期リターンしても
            // `session_guard` の `Drop` で `CFRelease` する。エラーパスは実機で誘発困難なため
            // 単体テストの対象外とし、このガードによる構造的な解放とコードレビューで担保する。
            // `Encoder::drop` は `VTCompressionSessionInvalidate` + `CFRelease` を行うため
            // エラーパスの解放方法とは非対称だが、ここで解放するセッションは一度も
            // エンコードしていない未使用のセッションであり、invalidate なしの `CFRelease`
            // のみで解放してよい。
            let session_guard = CfPtrMut(session);

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
            let status = sys::VTSessionSetProperties(session.cast(), properties_dict.0.cast());
            Error::check(status, "VTSessionSetProperties")?;

            // 成功パスではガードを forget して、`Encoder::drop` に解放を委ねる。
            // forget を書き忘れると `session_guard` の `Drop` が `CFRelease` し、
            // `Encoder::drop` の `VTCompressionSessionInvalidate` + `CFRelease` と合わせて
            // 二重解放や use-after-free になるため、成功パスでは必ず forget する。
            // forget の後で `Err` を返すコードを追加するとリークするため、この後に
            // エラーパスを追加しないこと。
            std::mem::forget(session_guard);
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
            let fps = config.fps_numerator.div_ceil(config.fps_denominator);
            push_expected_frame_rate_property(properties, cf_objects, fps)?;

            // ビットレート (指定時のみ設定)
            if let Some(bitrate) = config.average_bitrate {
                push_bitrate_property(properties, cf_objects, bitrate)?;
            }

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

            // データレートのハードリミット (指定時のみ設定。空 Vec は `Encoder::new` で `None` に正規化済み)
            if let Some(limits) = &config.data_rate_limits {
                push_data_rate_limits_property(properties, cf_objects, limits)?;
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
}
