//! FFI セッション生成とプロパティ構築 (CF オブジェクトの変換ヘルパーを含む)

use std::ffi::c_void;

use crate::{
    encoder::{
        EncodeCallbackContext, Encoder,
        config::{
            CodecConfig, DataRateLimit, EncoderConfig, H264EncoderConfig, H264EntropyMode,
            H264Profile, HevcEncoderConfig, HevcProfile,
        },
        handler::EncodeHandler,
    },
    error::Error,
    sys::{self, VTCompressionSessionCreate},
    types::{CfPtr, cf_array, cf_dictionary, cf_number_f64, cf_number_i32, cf_number_i64},
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

/// `VTCompressionSessionRef` の所有権を保持するガード
///
/// 保持するセッションは非 null であることを前提とする。`Drop` はセッションを無効化してから
/// 解放する。所有権を呼び出し元に移す場合は [`CompressionSessionGuard::into_raw`] を使うこと。
struct CompressionSessionGuard(sys::VTCompressionSessionRef);

impl CompressionSessionGuard {
    /// 保持しているセッションの所有権を呼び出し元に移し、生ポインタを返す
    ///
    /// `Drop` による破棄を行わないため、返り値のセッションは呼び出し元が無効化と解放を行う
    /// 責務を負う。
    fn into_raw(self) -> sys::VTCompressionSessionRef {
        let session = self.0;
        std::mem::forget(self);
        session
    }
}

impl Drop for CompressionSessionGuard {
    fn drop(&mut self) {
        // セッションを使い終えたら無効化してから解放する (Apple Developer Documentation
        // "VTCompressionSessionInvalidate(_:)" の Discussion: "call
        // VTCompressionSessionInvalidate to tear it down, and then call CFRelease to release
        // its memory")。同 Note にあるとおり、無効化によりセッションが決定的に破棄される。
        // https://developer.apple.com/documentation/videotoolbox/vtcompressionsessioninvalidate(_:)
        // この仕様は将来の OS / SDK の更新で変わりうるため、SDK を更新した際は再確認すること。
        unsafe {
            sys::VTCompressionSessionInvalidate(self.0);
            sys::CFRelease(self.0.cast());
        }
    }
}

impl<H: EncodeHandler> Encoder<H> {
    /// EncoderConfig と完了コールバックから VTCompressionSession を作成する
    ///
    /// 成功時はセッションの所有権が呼び出し元に移り、`Encoder::drop` が解放する。
    /// 失敗時は生成済みセッションを関数内で解放してから `Err` を返す。
    pub(super) unsafe fn create_compression_session(
        config: &EncoderConfig,
        callback_context: &EncodeCallbackContext<H>,
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
            // - `outputCallbackRefCon` には `EncodeCallbackContext<H>` のポインタを渡す。
            //   この構造体は `Box` でヒープに隔離されており、`Encoder` の生存期間中はアドレス不変である。
            // - 出力コールバック内で `&mut EncodeCallbackContext<H>` として復元し、
            //   ユーザー指定のハンドラを呼び出す (`process_encoded_output`)。
            let status = VTCompressionSessionCreate(
                std::ptr::null_mut(),
                config.width as i32,
                config.height as i32,
                codec_fourcc,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(callback),
                (callback_context as *const EncodeCallbackContext<H>)
                    .cast::<c_void>()
                    .cast_mut(),
                &mut session,
            );
            Error::check(status, "VTCompressionSessionCreate")?;

            // 生成したセッションをガードし、以降のプロパティ設定が失敗して早期リターンしても
            // `session_guard` の `Drop` が無効化と解放を行う。
            let session_guard = CompressionSessionGuard(session);

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

            let session = session_guard.into_raw();
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

            // データレートのハードリミット
            if !config.data_rate_limits.is_empty() {
                push_data_rate_limits_property(properties, cf_objects, &config.data_rate_limits)?;
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

#[cfg(test)]
mod tests {
    //! `CompressionSessionGuard` が保持するセッションを、`Drop` と `into_raw` の
    //! それぞれでどう扱うかを実 FFI で直接確認するためのテスト。
    //! セッションの生成と無効化の観測には Video Toolbox の API を直接呼ぶ必要があり、
    //! `tests/` から公開 API 経由では到達できないため、ここに置く。

    use super::*;

    /// `VTCompressionSessionCreate` に渡すためだけの出力コールバック
    ///
    /// 本テストはフレームを 1 枚も送らないため、このコールバックは呼び出されない。
    unsafe extern "C" fn noop_output_callback(
        _output_callback_ref_con: *mut c_void,
        _source_frame_ref_con: *mut c_void,
        _status: sys::OSStatus,
        _info_flags: sys::VTEncodeInfoFlags,
        _sample_buffer: sys::CMSampleBufferRef,
    ) {
    }

    /// テスト用の H.264 セッションを生成する
    ///
    /// 生成に失敗した場合はテストの前提が崩れているため panic する。
    /// 返り値のセッションの retain count は 1 で、呼び出し元が所有権を持つ。
    fn create_test_session() -> sys::VTCompressionSessionRef {
        unsafe {
            let mut session = std::ptr::null_mut();
            let status = sys::VTCompressionSessionCreate(
                std::ptr::null_mut(),
                320,
                240,
                u32::from_be_bytes(*b"avc1"),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(noop_output_callback),
                std::ptr::null_mut(),
                &mut session,
            );
            Error::check(status, "VTCompressionSessionCreate")
                .expect("テスト用の圧縮セッションを生成できなかった");
            assert!(
                !session.is_null(),
                "VTCompressionSessionCreate が NULL を返した"
            );
            session
        }
    }

    /// セッションにプロパティを設定し、その戻り値を返す
    ///
    /// 無効化済みのセッションでは `kVTInvalidSessionErr` が返るため、
    /// この戻り値でセッションが無効化されているかどうかを観測できる。
    fn set_property_status(session: sys::VTCompressionSessionRef) -> sys::OSStatus {
        unsafe {
            sys::VTSessionSetProperty(
                session.cast(),
                sys::kVTCompressionPropertyKey_RealTime,
                sys::kCFBooleanTrue.cast(),
            )
        }
    }

    /// ガードの `Drop` がセッションを無効化すること
    #[test]
    fn compression_session_guard_invalidates_session_on_drop() {
        let session = create_test_session();

        // ガードの `Drop` がセッションを解放してもオブジェクトを観測できるよう、
        // テスト側で参照を 1 つ余分に保持する。
        unsafe { sys::CFRetain(session.cast()) };

        // ガードの `Drop` でセッションが無効化され、解放される
        drop(CompressionSessionGuard(session));

        assert_eq!(
            set_property_status(session),
            sys::kVTInvalidSessionErr,
            "Drop したガードが保持していたセッションは無効化されていること"
        );

        // テスト側で保持していた参照を解放する
        unsafe { sys::CFRelease(session.cast()) };
    }

    /// `into_raw` がセッションを無効化せずに所有権を移すこと
    #[test]
    fn compression_session_guard_into_raw_keeps_session_valid() {
        let session = create_test_session();

        let guard = CompressionSessionGuard(session);
        let transferred = guard.into_raw();
        assert_eq!(
            transferred, session,
            "into_raw は保持していたセッションをそのまま返すこと"
        );

        assert_eq!(
            set_property_status(session),
            0,
            "into_raw はセッションを無効化してはならない"
        );

        // 所有権は呼び出し元に移っているため、テスト側でセッションを破棄する
        unsafe {
            sys::VTCompressionSessionInvalidate(session);
            sys::CFRelease(session.cast());
        }
    }
}
