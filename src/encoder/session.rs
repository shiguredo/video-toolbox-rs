//! FFI セッション生成とプロパティ構築 (CF オブジェクトの変換ヘルパーを含む)

use std::ffi::c_void;

use crate::{
    encoder::{
        EncodeCallbackContext, Encoder,
        config::{
            CodecConfig, DataRateLimit, EncoderConfig, H264EntropyMode, H264Profile, HevcProfile,
        },
        handler::EncodeHandler,
    },
    error::Error,
    sys::{self, VTCompressionSessionCreate},
    types::{CfPtr, cf_array, cf_boolean, cf_number_f64, cf_number_i32, cf_number_i64},
};

/// `data_rate_limits` を CFArray に変換する
///
/// kVTCompressionPropertyKey_DataRateLimits は「bytes, seconds を交互に並べた偶数個の
/// CFNumber の CFArray」と規定されている (VTCompressionProperties.h)。
/// CFArray は各要素を retain するため、要素の CFNumber は本関数内で drop してよい。
fn data_rate_limits_array(limits: &[DataRateLimit]) -> Result<CfPtr<c_void>, Error> {
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
    cf_array(&raw)
}

/// セッションにプロパティを 1 個設定し、Video Toolbox が受け付けなければエラーを返す
///
/// `property_name` は失敗時に [`Error::VideoToolbox`] へ入れる名前であり、
/// `property` に渡す `kVTCompressionPropertyKey_*` の定数名と揃える。
/// `value` は呼び出しの間だけ有効であればよく、Video Toolbox が retain する。
///
/// # Safety
///
/// `session` は有効な圧縮セッションで、`property` と `value` は `VTSessionSetProperty` が
/// 要求する型の CF オブジェクトでなければならない。
unsafe fn set_property(
    session: sys::VTCompressionSessionRef,
    property: sys::CFStringRef,
    property_name: &'static str,
    value: *const c_void,
) -> Result<(), Error> {
    // SAFETY: 呼び出し側が上記の前提を満たしている。
    let status = unsafe { sys::VTSessionSetProperty(session.cast(), property, value) };
    Error::check_property(status, property_name)
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
                CodecConfig::Hevc(hevc) => (
                    u32::from_be_bytes(*b"hvc1"),
                    Self::output_callback_h265 as unsafe extern "C" fn(_, _, _, _, _),
                    hevc.profile.map(|profile| match profile {
                        HevcProfile::Main => sys::kVTProfileLevel_HEVC_Main_AutoLevel,
                        HevcProfile::Main10 => sys::kVTProfileLevel_HEVC_Main10_AutoLevel,
                    }),
                ),
                CodecConfig::H264(h264) => (
                    u32::from_be_bytes(*b"avc1"),
                    Self::output_callback_h264 as unsafe extern "C" fn(_, _, _, _, _),
                    h264.profile.map(|profile| match profile {
                        H264Profile::Baseline => sys::kVTProfileLevel_H264_Baseline_AutoLevel,
                        H264Profile::Main => sys::kVTProfileLevel_H264_Main_AutoLevel,
                        H264Profile::High => sys::kVTProfileLevel_H264_High_AutoLevel,
                    }),
                ),
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

            Self::set_properties(session, config, profile_level)?;

            let session = session_guard.into_raw();
            Ok(session)
        }
    }

    /// `EncoderConfig` のプロパティをセッションに設定する
    ///
    /// `config` で `None` のフィールド、空の `data_rate_limits`、`profile_level` が `None` の
    /// プロファイルレベルは設定しない。設定できなかったプロパティがあれば、その名前を含む
    /// [`Error::VideoToolbox`] を返す。
    ///
    /// # Safety
    ///
    /// `session` は `VTCompressionSessionCreate` で生成した有効なセッションでなければならない。
    unsafe fn set_properties(
        session: sys::VTCompressionSessionRef,
        config: &EncoderConfig,
        profile_level: Option<sys::CFStringRef>,
    ) -> Result<(), Error> {
        // プロパティは 1 個ずつ設定し、その戻り値を確認する。Video Toolbox は選択された
        // エンコーダーが対応していないプロパティを辞書にまとめて渡された場合でも成功を返すため、
        // 受け付けられなかった指定を `Encoder::new` のエラーにするには 1 個ずつの戻り値が要る。
        // SAFETY: `session` は呼び出し元が渡した有効なセッションであり、プロパティのキーと値は
        // Video Toolbox が定義する `CFStringRef` / `CFBoolean` / `CFNumber` である。
        unsafe {
            // プロファイルレベル
            if let Some(profile_level) = profile_level {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_ProfileLevel,
                    "kVTCompressionPropertyKey_ProfileLevel",
                    profile_level.cast(),
                )?;
            }

            // フレームレート (PTS の計算にも使うため、常に設定する)
            let fps = config.fps_numerator.div_ceil(config.fps_denominator);
            let fps_value = cf_number_i32(fps as i32)?;
            set_property(
                session,
                sys::kVTCompressionPropertyKey_ExpectedFrameRate,
                "kVTCompressionPropertyKey_ExpectedFrameRate",
                fps_value.0,
            )?;

            // 平均ビットレート
            if let Some(bitrate) = config.average_bitrate {
                let bitrate_value = cf_number_i64(bitrate as i64)?;
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_AverageBitRate,
                    "kVTCompressionPropertyKey_AverageBitRate",
                    bitrate_value.0,
                )?;
            }

            // 速度優先モード
            if let Some(prioritize) = config.prioritize_encoding_speed_over_quality {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
                    "kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality",
                    cf_boolean(prioritize),
                )?;
            }

            // リアルタイムモード
            if let Some(real_time) = config.real_time {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_RealTime,
                    "kVTCompressionPropertyKey_RealTime",
                    cf_boolean(real_time),
                )?;
            }

            // 電力効率最大化
            if let Some(maximize_power_efficiency) = config.maximize_power_efficiency {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_MaximizePowerEfficiency,
                    "kVTCompressionPropertyKey_MaximizePowerEfficiency",
                    cf_boolean(maximize_power_efficiency),
                )?;
            }

            // フレーム再順序付け
            if let Some(allow_frame_reordering) = config.allow_frame_reordering {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_AllowFrameReordering,
                    "kVTCompressionPropertyKey_AllowFrameReordering",
                    cf_boolean(allow_frame_reordering),
                )?;
            }

            // 時間的圧縮
            if let Some(allow_temporal_compression) = config.allow_temporal_compression {
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_AllowTemporalCompression,
                    "kVTCompressionPropertyKey_AllowTemporalCompression",
                    cf_boolean(allow_temporal_compression),
                )?;
            }

            // キーフレーム間隔 (フレーム数)
            if let Some(interval) = config.max_key_frame_interval {
                let interval_value = cf_number_i32(interval.get() as i32)?;
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    "kVTCompressionPropertyKey_MaxKeyFrameInterval",
                    interval_value.0,
                )?;
            }

            // キーフレーム間隔 (秒数)
            if let Some(duration) = config.max_key_frame_interval_duration {
                let duration_value = cf_number_f64(duration.as_secs_f64())?;
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
                    "kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration",
                    duration_value.0,
                )?;
            }

            // フレーム遅延制限
            if let Some(delay_count) = config.max_frame_delay_count {
                let delay_value = cf_number_i32(delay_count.get() as i32)?;
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_MaxFrameDelayCount,
                    "kVTCompressionPropertyKey_MaxFrameDelayCount",
                    delay_value.0,
                )?;
            }

            // データレートのハードリミット
            if !config.data_rate_limits.is_empty() {
                let limits = data_rate_limits_array(&config.data_rate_limits)?;
                set_property(
                    session,
                    sys::kVTCompressionPropertyKey_DataRateLimits,
                    "kVTCompressionPropertyKey_DataRateLimits",
                    limits.0,
                )?;
            }

            // コーデック固有の設定
            match &config.codec {
                CodecConfig::Hevc(hevc) => {
                    if let Some(allow_open_gop) = hevc.allow_open_gop {
                        set_property(
                            session,
                            sys::kVTCompressionPropertyKey_AllowOpenGOP,
                            "kVTCompressionPropertyKey_AllowOpenGOP",
                            cf_boolean(allow_open_gop),
                        )?;
                    }
                }
                CodecConfig::H264(h264) => {
                    if let Some(entropy_mode) = h264.entropy_mode {
                        let entropy_mode = match entropy_mode {
                            H264EntropyMode::Cavlc => sys::kVTH264EntropyMode_CAVLC,
                            H264EntropyMode::Cabac => sys::kVTH264EntropyMode_CABAC,
                        };
                        set_property(
                            session,
                            sys::kVTCompressionPropertyKey_H264EntropyMode,
                            "kVTCompressionPropertyKey_H264EntropyMode",
                            entropy_mode.cast(),
                        )?;
                    }
                }
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
