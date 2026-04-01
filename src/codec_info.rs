//! コーデック情報の照会

use std::ffi::c_void;

use crate::{CfPtr, sys};

/// コーデック種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodecType {
    /// H.264 / AVC
    H264,
    /// H.265 / HEVC
    Hevc,
    /// VP9
    Vp9,
    /// AV1
    Av1,
}

impl VideoCodecType {
    /// CMVideoCodecType (FourCC) に変換する
    fn to_fourcc(self) -> u32 {
        match self {
            Self::H264 => u32::from_be_bytes(*b"avc1"),
            Self::Hevc => u32::from_be_bytes(*b"hvc1"),
            Self::Vp9 => u32::from_be_bytes(*b"vp09"),
            Self::Av1 => u32::from_be_bytes(*b"av01"),
        }
    }

    /// すべてのコーデック種別を返す
    fn all() -> &'static [Self] {
        &[Self::H264, Self::Hevc, Self::Vp9, Self::Av1]
    }
}

/// コーデックごとの情報
#[derive(Debug, Clone, PartialEq)]
pub struct CodecInfo {
    /// コーデック種別
    pub codec: VideoCodecType,
    /// デコード情報
    pub decoding: DecodingInfo,
    /// エンコード情報
    pub encoding: EncodingInfo,
}

/// デコード情報
///
/// `supported` は `VTIsHardwareDecodeSupported` に基づく **ハードウェアデコード可否**である。
/// ソフトウェアデコードのみ利用可能な環境では `false` になり得るが、**システム全体がデコード不能**という意味ではない。
/// `hardware_accelerated` は現状 `supported` と同じ値になる（内部の `probe_decoding` 実装）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodingInfo {
    /// ハードウェアデコードが可能か（`VTIsHardwareDecodeSupported`）
    pub supported: bool,
    /// ハードウェアアクセラレーションが利用可能か（現状 `supported` と同じ）
    pub hardware_accelerated: bool,
}

/// エンコード情報
#[derive(Debug, Clone, PartialEq)]
pub struct EncodingInfo {
    /// エンコードが可能か
    pub supported: bool,
    /// ハードウェアアクセラレーションが利用可能か
    pub hardware_accelerated: bool,
    /// フレームリオーダリング（B フレーム）をサポートするか
    pub supports_frame_reordering: bool,
    /// マルチパスエンコードをサポートするか
    pub supports_multi_pass: bool,
    /// コーデック固有のプロファイル情報
    pub profiles: EncodingProfiles,
}

/// コーデック固有のエンコードプロファイル情報
///
/// 現在は H.264 と HEVC のプロファイルのみ対応している。
/// VideoToolbox が VP9 / AV1 エンコードに対応した場合はバリアントを追加する。
#[derive(Debug, Clone, PartialEq)]
pub enum EncodingProfiles {
    /// H.264 プロファイル一覧
    H264(Vec<H264EncodingProfile>),
    /// HEVC プロファイル一覧
    Hevc(Vec<HevcEncodingProfile>),
    /// プロファイル情報なし（エンコード非対応）
    None,
}

/// H.264 エンコードプロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264EncodingProfile {
    /// Baseline
    Baseline,
    /// Constrained Baseline
    ConstrainedBaseline,
    /// Main
    Main,
    /// High
    High,
    /// Constrained High
    ConstrainedHigh,
}

/// HEVC エンコードプロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HevcEncodingProfile {
    /// Main
    Main,
    /// Main10
    Main10,
    /// Main 4:2:2 10-bit
    Main42210,
}

/// このバックエンドで利用可能なコーデック情報の一覧を返す
///
/// 各要素の [`CodecInfo::decoding`] は [`DecodingInfo`] を参照する。デコード可否の意味は [`DecodingInfo`] の rustdoc を参照すること。
#[cfg(target_os = "macos")]
pub fn supported_codecs() -> Vec<CodecInfo> {
    VideoCodecType::all()
        .iter()
        .map(|&codec| CodecInfo {
            codec,
            decoding: probe_decoding(codec),
            encoding: probe_encoding(codec),
        })
        .collect()
}

/// `VTIsHardwareDecodeSupported` でデコード情報を判定する
///
/// 返却する `supported` / `hardware_accelerated` は **ハードウェアパス**の可否に対応する。
#[cfg(target_os = "macos")]
fn probe_decoding(codec: VideoCodecType) -> DecodingInfo {
    let fourcc = codec.to_fourcc();
    let hw = unsafe { sys::VTIsHardwareDecodeSupported(fourcc) != 0 };

    DecodingInfo {
        supported: hw,
        hardware_accelerated: hw,
    }
}

/// VTCopyVideoEncoderList と VTCopySupportedPropertyDictionaryForEncoder から
/// エンコード情報を判定する
#[cfg(target_os = "macos")]
fn probe_encoding(codec: VideoCodecType) -> EncodingInfo {
    let target_fourcc = codec.to_fourcc();
    let mut supported = false;
    let mut hardware_accelerated = false;
    let mut supports_frame_reordering = false;
    let mut supports_multi_pass = false;

    unsafe {
        let mut encoder_list: sys::CFArrayRef = std::ptr::null_mut();
        let status = sys::VTCopyVideoEncoderList(std::ptr::null_mut(), &mut encoder_list);
        if status != 0 || encoder_list.is_null() {
            return EncodingInfo {
                supported: false,
                hardware_accelerated: false,
                supports_frame_reordering: false,
                supports_multi_pass: false,
                profiles: EncodingProfiles::None,
            };
        }
        let _list_guard = CfPtr(encoder_list as *const c_void);

        let count = sys::CFArrayGetCount(encoder_list);
        for i in 0..count {
            let entry = sys::CFArrayGetValueAtIndex(encoder_list, i);
            if entry.is_null() {
                continue;
            }
            // 配列要素が辞書でない場合はスキップ（NULL チェックだけでは型は保証されない）
            if sys::CFGetTypeID(entry as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
                continue;
            }
            let entry = entry as sys::CFDictionaryRef;

            // kVTVideoEncoderList_CodecType から FourCC を取得する
            let codec_type_value = sys::CFDictionaryGetValue(
                entry,
                sys::kVTVideoEncoderList_CodecType as *const c_void,
            );
            if codec_type_value.is_null() {
                continue;
            }
            if sys::CFGetTypeID(codec_type_value as sys::CFTypeRef) != sys::CFNumberGetTypeID() {
                continue;
            }

            let mut fourcc: u32 = 0;
            let ok = sys::CFNumberGetValue(
                codec_type_value.cast(),
                sys::kCFNumberSInt32Type as sys::CFNumberType,
                (&mut fourcc as *mut u32).cast(),
            );
            if ok == 0 || fourcc != target_fourcc {
                continue;
            }

            // このコーデックのエンコーダーが見つかった
            supported = true;

            // いずれかのエンコーダーが対応していれば true とする
            if get_cf_bool(entry, sys::kVTVideoEncoderList_IsHardwareAccelerated) {
                hardware_accelerated = true;
            }
            if get_cf_bool(entry, sys::kVTVideoEncoderList_SupportsFrameReordering) {
                supports_frame_reordering = true;
            }
            if get_cf_bool(entry, sys::kVTVideoEncoderList_SupportsMultiPass) {
                supports_multi_pass = true;
            }
        }
    }

    let profiles = if supported {
        query_encoding_profiles(codec, target_fourcc)
    } else {
        EncodingProfiles::None
    };

    EncodingInfo {
        supported,
        hardware_accelerated,
        supports_frame_reordering,
        supports_multi_pass,
        profiles,
    }
}

#[cfg(target_os = "macos")]
unsafe fn get_cf_bool(dict: sys::CFDictionaryRef, key: sys::CFStringRef) -> bool {
    unsafe {
        let value = sys::CFDictionaryGetValue(dict, key as *const c_void);
        !value.is_null() && value == sys::kCFBooleanTrue as *const c_void
    }
}

/// プロファイル照会には解像度の指定が必要なため 1920x1080 を代表値として使用する。
/// 解像度固有のプロファイル制約は反映されない。
#[cfg(target_os = "macos")]
fn query_encoding_profiles(codec: VideoCodecType, fourcc: u32) -> EncodingProfiles {
    unsafe {
        let mut props: sys::CFDictionaryRef = std::ptr::null_mut();
        let status = sys::VTCopySupportedPropertyDictionaryForEncoder(
            1920,
            1080,
            fourcc,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut props,
        );
        if status != 0 || props.is_null() {
            return EncodingProfiles::None;
        }
        if sys::CFGetTypeID(props as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
            return EncodingProfiles::None;
        }
        let _props_guard = CfPtr(props as *const c_void);

        let profile_entry = sys::CFDictionaryGetValue(
            props,
            sys::kVTCompressionPropertyKey_ProfileLevel as *const c_void,
        );
        if profile_entry.is_null() {
            return EncodingProfiles::None;
        }
        if sys::CFGetTypeID(profile_entry) != sys::CFDictionaryGetTypeID() {
            return EncodingProfiles::None;
        }

        let value_list = sys::CFDictionaryGetValue(
            profile_entry as sys::CFDictionaryRef,
            sys::kVTPropertySupportedValueListKey as *const c_void,
        );
        if value_list.is_null() {
            return EncodingProfiles::None;
        }
        if sys::CFGetTypeID(value_list as sys::CFTypeRef) != sys::CFArrayGetTypeID() {
            return EncodingProfiles::None;
        }

        let value_array = value_list as sys::CFArrayRef;
        let count = sys::CFArrayGetCount(value_array);

        // CFStringRef のライフタイムは props が有効な間のみ有効なので、
        // マッチングはこのスコープ内で完了させる
        match codec {
            VideoCodecType::H264 => {
                let map = &[
                    (
                        sys::kVTProfileLevel_H264_Baseline_AutoLevel,
                        H264EncodingProfile::Baseline,
                    ),
                    (
                        sys::kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel,
                        H264EncodingProfile::ConstrainedBaseline,
                    ),
                    (
                        sys::kVTProfileLevel_H264_Main_AutoLevel,
                        H264EncodingProfile::Main,
                    ),
                    (
                        sys::kVTProfileLevel_H264_High_AutoLevel,
                        H264EncodingProfile::High,
                    ),
                    (
                        sys::kVTProfileLevel_H264_ConstrainedHigh_AutoLevel,
                        H264EncodingProfile::ConstrainedHigh,
                    ),
                ];
                EncodingProfiles::H264(match_profiles(value_array, count, map))
            }
            VideoCodecType::Hevc => {
                let map = &[
                    (
                        sys::kVTProfileLevel_HEVC_Main_AutoLevel,
                        HevcEncodingProfile::Main,
                    ),
                    (
                        sys::kVTProfileLevel_HEVC_Main10_AutoLevel,
                        HevcEncodingProfile::Main10,
                    ),
                    (
                        sys::kVTProfileLevel_HEVC_Main42210_AutoLevel,
                        HevcEncodingProfile::Main42210,
                    ),
                ];
                EncodingProfiles::Hevc(match_profiles(value_array, count, map))
            }
            _ => EncodingProfiles::None,
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn match_profiles<T: Copy + PartialEq>(
    value_array: sys::CFArrayRef,
    count: sys::CFIndex,
    map: &[(sys::CFStringRef, T)],
) -> Vec<T> {
    let mut profiles = Vec::new();
    for i in 0..count {
        let value = unsafe { sys::CFArrayGetValueAtIndex(value_array, i) };
        if value.is_null() {
            continue;
        }
        if unsafe { sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFStringGetTypeID() } {
            continue;
        }
        let value = value as sys::CFStringRef;
        for &(ref_str, profile) in map {
            if unsafe { sys::CFEqual(value as sys::CFTypeRef, ref_str as sys::CFTypeRef) } != 0
                && !profiles.contains(&profile)
            {
                profiles.push(profile);
            }
        }
    }
    profiles
}
