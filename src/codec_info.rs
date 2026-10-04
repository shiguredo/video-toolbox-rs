//! コーデック情報の照会

use std::ffi::c_void;

use crate::{sys, types::CfPtr};

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
    /// このコーデックで利用できるエンコーダーの一覧
    ///
    /// `VTCopyVideoEncoderList` が返す順序のまま格納する。空の場合はエンコード非対応。
    pub encoders: Vec<EncodingInfo>,
}

/// デコード情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodingInfo {
    /// ハードウェアデコードが可能か（`VTIsHardwareDecodeSupported`）
    pub hardware_accelerated: bool,
}

/// エンコーダー 1 つの情報
///
/// `VTCopyVideoEncoderList` が返すエンコーダー一覧のエントリ 1 件に対応する。
/// どのエンコーダーが使われるかは解像度によって変わるため、解像度が決まっている場合は
/// [`EncodingCapabilities`] を照会すること。
#[derive(Debug, Clone, PartialEq)]
pub struct EncodingInfo {
    /// エンコーダーの ID（`kVTVideoEncoderList_EncoderID`）
    ///
    /// `kVTVideoEncoderSpecification_EncoderID` にそのまま指定できる逆 DNS 形式の識別子
    /// (例: `"com.apple.videotoolbox.videoencoder.ave.avc"`)。
    pub encoder_id: String,
    /// エンコーダーの表示名（`kVTVideoEncoderList_EncoderName`）
    ///
    /// 例: `"Apple H.264 (HW)"`。利用者に見せるための文字列であり、機械的な判定には
    /// [`EncodingInfo::encoder_id`] を使うこと。取得できなかった場合は `None`。
    pub encoder_name: Option<String>,
    /// エンコーダーが扱うコーデックの表示名（`kVTVideoEncoderList_CodecName`）
    ///
    /// 例: `"H.264"`。利用者に見せるための文字列であり、機械的な判定には
    /// [`CodecInfo::codec`] を使うこと。取得できなかった場合は `None`。
    pub codec_name: Option<String>,
    /// このエンコーダーがハードウェア実装か（`kVTVideoEncoderList_IsHardwareAccelerated`）
    ///
    /// キーが無い場合は false として扱う (キーが無いエンコーダーはハードウェアではない)。
    pub hardware_accelerated: bool,
    /// このエンコーダーがフレームリオーダリング（B フレーム）に対応するか
    /// （`kVTVideoEncoderList_SupportsFrameReordering`）
    ///
    /// `kVTVideoEncoderList_SupportsFrameReordering` はキーが無い場合に true と見なす仕様
    /// (VTVideoEncoderList.h) のため、キーが無い場合は対応しているものとして true になる。
    pub supports_frame_reordering: bool,
    /// このエンコーダーがマルチパスエンコードに対応するか
    /// （`kVTVideoEncoderList_SupportsMultiPass`）
    ///
    /// `kVTVideoEncoderList_SupportsMultiPass` はキーが無い場合に false と見なす仕様
    /// (VTVideoEncoderList.h) のため、キーが無い場合は非対応として false になる。
    /// このキーは macOS では利用できない (VTVideoEncoderList.h の API_UNAVAILABLE) ため、
    /// macOS では常に false になる。
    pub supports_multi_pass: bool,
    /// エンコーダーの性能指標（`kVTVideoEncoderList_PerformanceRating`）
    ///
    /// 同じフォーマットの他のエンコーダーと比較した相対値であり、絶対的な性能値ではない。
    /// Apple は値の範囲を定義していないため、同じ環境で得た値同士の大小比較にのみ使うこと。
    /// キーが存在しない、または取得できなかった場合は `None`。
    pub performance_rating: Option<f64>,
    /// エンコーダーの品質指標（`kVTVideoEncoderList_QualityRating`）
    ///
    /// 同じフォーマットの他のエンコーダーと比較した相対値であり、解像度やビットレートに
    /// よって優劣が入れ替わりうる一般化された値である。Apple は値の範囲を定義していないため、
    /// 同じ環境で得た値同士の大小比較にのみ使うこと。
    /// キーが存在しない、または取得できなかった場合は `None`。
    pub quality_rating: Option<f64>,
    /// エンコーダーにグローバルなインスタンス上限があるか
    /// （`kVTVideoEncoderList_InstanceLimit`）
    ///
    /// `Some(true)` は、同時に生成できるエンコーダー数に上限があり、
    /// エンコーダーが一時的に利用できなくなりうることを表す。
    /// キーが存在しない場合は `None` になる
    /// (キーが無いことを「上限なし」と解釈してはならない)。
    pub has_instance_limit: Option<bool>,
}

/// コーデック固有のエンコードプロファイル情報
#[derive(Debug, Clone, PartialEq)]
pub enum EncodingProfiles {
    /// H.264 プロファイル一覧
    H264(Vec<H264EncodingProfile>),
    /// HEVC プロファイル一覧
    Hevc(Vec<HevcEncodingProfile>),
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

/// `query_encoding_capabilities()` が返す結果
#[derive(Debug, Clone, PartialEq)]
pub struct EncodingCapabilities {
    /// 照会で選ばれたエンコーダー
    ///
    /// [`CodecInfo::encoders`] のいずれかの要素である。ハードウェアエンコーダーが使える
    /// 解像度ではハードウェアのエントリ、使えない解像度ではソフトウェアのエントリになる。
    pub encoder: EncodingInfo,
    /// 選ばれたエンコーダーが扱うプロファイルの一覧
    ///
    /// Video Toolbox がプロファイルレベルに指定できる値のうち、本クレートが
    /// [`H264EncodingProfile`] / [`HevcEncodingProfile`] として表現できるものを格納する。
    /// Video Toolbox はこれ以外の値も返しうる (H.264 の High 4:2:2 / High 4:4:4 Predictive、
    /// HEVC の 4:4:4 系や Monochrome 系など) ため、ここに含まれないことが非対応を意味するとは限らない。
    ///
    /// Video Toolbox がプロファイル一覧を返さなかった場合は `None`
    /// (`None` はエンコード非対応を意味しない)。
    pub profiles: Option<EncodingProfiles>,
}

/// このバックエンドで利用可能なコーデック情報の一覧を返す
#[cfg(target_os = "macos")]
pub fn supported_codecs() -> Vec<CodecInfo> {
    VideoCodecType::all()
        .iter()
        .map(|&codec| CodecInfo {
            codec,
            decoding: probe_decoding(codec),
            encoders: encoder_infos(codec),
        })
        .collect()
}

/// 指定したコーデックのデコード情報を返す
#[cfg(target_os = "macos")]
fn probe_decoding(codec: VideoCodecType) -> DecodingInfo {
    let hardware_accelerated = unsafe { sys::VTIsHardwareDecodeSupported(codec.to_fourcc()) != 0 };

    DecodingInfo {
        hardware_accelerated,
    }
}

/// 指定したコーデック・解像度でエンコードするときの情報を返す
///
/// 解像度によって選ばれるエンコーダーが変わるため、エンコードする解像度が決まっている場合は
/// [`supported_codecs`] ではなくこの関数を使う。
///
/// 解像度に対応するハードウェアエンコーダーがあればそれを、無ければソフトウェアエンコーダーを
/// 選ぶ既定の選択で選ばれたエンコーダーの情報を返す。返る
/// [`EncodingInfo::hardware_accelerated`] が、指定した解像度でハードウェアエンコーダーが使える
/// かどうかを表す。
///
/// 次の場合は `None` を返す。
///
/// - コーデックにエンコーダーが無い場合（VP9 / AV1）
/// - `width` または `height` が 0 の場合と、`i32` に収まらない場合
/// - 照会に失敗した場合と、選ばれたエンコーダーを一覧から特定できなかった場合
#[cfg(target_os = "macos")]
pub fn query_encoding_capabilities(
    codec: VideoCodecType,
    width: u32,
    height: u32,
) -> Option<EncodingCapabilities> {
    // 解像度は Video Toolbox の引数に合わせて i32 で扱う。
    // 0 以下は Video Toolbox が -12902 (kVTParameterErr) を返すため、下の照会失敗として扱われる
    let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
        return None;
    };

    let query = query_encoder_properties(codec.to_fourcc(), width, height)?;

    // 選ばれたエンコーダーの属性をエンコーダー一覧から引く。一覧から特定できない場合は
    // エンコーダー ID 以外の情報を返せないため、情報が取得できなかったものとして扱う。
    let encoder = find_encoder_info(codec, &query.encoder_id)?;

    // プロファイルは同じサポートプロパティ辞書から読み出す
    let profiles = query_encoding_profiles(codec, query.properties.as_ref());

    Some(EncodingCapabilities { encoder, profiles })
}

/// `VTCopySupportedPropertyDictionaryForEncoder` の照会結果
///
/// `properties` が保持する CFDictionary はこの値が生存している間のみ有効なため、
/// 辞書から文字列や数値を読み出す処理はこの値の生存期間内に完了させること。
#[cfg(target_os = "macos")]
#[derive(Debug)]
struct EncoderQueryResult {
    /// 選択されたエンコーダーの ID
    encoder_id: String,
    /// 選択されたエンコーダーのサポートプロパティ辞書
    properties: Option<CfPtr<c_void>>,
}

/// 指定したコーデック・解像度で選ばれるエンコーダーの ID と、そのエンコーダーのサポート
/// プロパティ辞書を返す
///
/// ハードウェアエンコーダーがあればそれを、無ければソフトウェアエンコーダーを選ぶ既定の選択の
/// 結果を返す。サポートプロパティ辞書を取得できなかった場合は `properties` を `None` にして
/// エンコーダーの ID だけを返す。
///
/// 照会に失敗した場合と、選ばれたエンコーダーの ID を取得できなかった場合は `None` を返す。
#[cfg(target_os = "macos")]
fn query_encoder_properties(fourcc: u32, width: i32, height: i32) -> Option<EncoderQueryResult> {
    unsafe {
        let mut encoder_id: sys::CFStringRef = std::ptr::null_mut();
        let mut properties: sys::CFDictionaryRef = std::ptr::null_mut();
        let status = sys::VTCopySupportedPropertyDictionaryForEncoder(
            width,
            height,
            fourcc,
            std::ptr::null_mut(),
            &mut encoder_id,
            &mut properties,
        );
        if status != 0 {
            // 失敗した場合は出力引数が設定されない (NULL で初期化した状態のままになる) が、
            // 非 NULL が返った場合に備えて解放してから戻す
            if !encoder_id.is_null() {
                sys::CFRelease(encoder_id.cast());
            }
            if !properties.is_null() {
                sys::CFRelease(properties.cast());
            }
            return None;
        }

        // 型チェックより前にガードを生成し、失敗パスでのリークを防ぐ
        let properties = if properties.is_null() {
            None
        } else {
            Some(CfPtr(properties as *const c_void))
        };
        if encoder_id.is_null() {
            // エンコーダー ID を取得できない場合は、エンコーダーを特定できなかったものとして扱う
            return None;
        }
        let encoder_id = {
            let _encoder_id_guard = CfPtr(encoder_id as *const c_void);
            cf_string_to_string(encoder_id)?
        };

        Some(EncoderQueryResult {
            encoder_id,
            properties,
        })
    }
}

/// `VTCopyVideoEncoderList` が返すエンコーダー一覧のうち、対象コーデックのエントリ
///
/// `entries` の要素は `_list` が保持する CFArray が生存している間だけ有効なため、
/// この値を保持している間だけ参照すること。
#[cfg(target_os = "macos")]
struct EncoderList {
    /// `VTCopyVideoEncoderList` が返した配列
    ///
    /// `entries` を有効に保つためだけに保持する。
    _list: CfPtr<c_void>,
    /// 対象コーデックのエントリ (CFDictionary)
    entries: Vec<sys::CFDictionaryRef>,
}

/// `VTCopyVideoEncoderList` が返す一覧から、指定したコーデックのエントリだけを抜き出す
///
/// 辞書でない要素は格納しない。一覧の取得に失敗した場合と、対象コーデックのエントリが
/// 1 つも無い場合は `None` を返す。
#[cfg(target_os = "macos")]
fn encoder_list(fourcc: u32) -> Option<EncoderList> {
    unsafe {
        let mut list: sys::CFArrayRef = std::ptr::null_mut();
        let status = sys::VTCopyVideoEncoderList(std::ptr::null_mut(), &mut list);
        if status != 0 || list.is_null() {
            // 失敗した場合は出力引数が設定されない (NULL で初期化した状態のままになる) が、
            // 非 NULL が返った場合に備えて解放してから戻す
            if !list.is_null() {
                sys::CFRelease(list.cast());
            }
            return None;
        }
        let list_guard = CfPtr(list as *const c_void);

        let mut entries = Vec::new();
        let count = sys::CFArrayGetCount(list);
        for i in 0..count {
            let entry = sys::CFArrayGetValueAtIndex(list, i);
            if entry.is_null() {
                continue;
            }
            // 配列要素が辞書でない場合はスキップする
            if sys::CFGetTypeID(entry as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
                continue;
            }
            let entry = entry as sys::CFDictionaryRef;
            if encoder_fourcc(entry) != Some(fourcc) {
                continue;
            }
            entries.push(entry);
        }
        if entries.is_empty() {
            return None;
        }

        Some(EncoderList {
            _list: list_guard,
            entries,
        })
    }
}

/// 対象コーデックのエンコーダー一覧を返す
///
/// コーデックにエンコーダーが無い場合と、一覧の取得に失敗した場合は空の `Vec` を返す。
#[cfg(target_os = "macos")]
fn encoder_infos(codec: VideoCodecType) -> Vec<EncodingInfo> {
    let Some(list) = encoder_list(codec.to_fourcc()) else {
        return Vec::new();
    };

    list.entries
        .iter()
        .filter_map(|&entry| unsafe { encoding_info(entry) })
        .collect()
}

/// `VTCopyVideoEncoderList` のエントリ 1 件からエンコーダーの情報を読み出す
///
/// エンコーダーを識別する `kVTVideoEncoderList_EncoderID` を取得できないエントリは、
/// エンコーダーとして扱えないため `None` を返す。
#[cfg(target_os = "macos")]
unsafe fn encoding_info(entry: sys::CFDictionaryRef) -> Option<EncodingInfo> {
    unsafe {
        let encoder_id = get_cf_string(entry, sys::kVTVideoEncoderList_EncoderID)?;

        Some(EncodingInfo {
            encoder_id,
            encoder_name: get_cf_string(entry, sys::kVTVideoEncoderList_EncoderName),
            codec_name: get_cf_string(entry, sys::kVTVideoEncoderList_CodecName),
            // kVTVideoEncoderList_IsHardwareAccelerated は「存在して true のとき」に
            // ハードウェアエンコーダーを表すため、キーが無い場合は false として扱う。
            hardware_accelerated: get_cf_bool(
                entry,
                sys::kVTVideoEncoderList_IsHardwareAccelerated,
            )
            .unwrap_or(false),
            // kVTVideoEncoderList_SupportsFrameReordering はキーが無い場合に true と見なす仕様
            // (VTVideoEncoderList.h) のため、キーが無い場合は対応しているものとして扱う。
            supports_frame_reordering: get_cf_bool(
                entry,
                sys::kVTVideoEncoderList_SupportsFrameReordering,
            )
            .unwrap_or(true),
            // kVTVideoEncoderList_SupportsMultiPass はキーが無い場合に false と見なす仕様のため、
            // キーが無い場合は非対応として扱う。
            supports_multi_pass: get_cf_bool(entry, sys::kVTVideoEncoderList_SupportsMultiPass)
                .unwrap_or(false),
            performance_rating: get_cf_number_f64(
                entry,
                sys::kVTVideoEncoderList_PerformanceRating,
            ),
            quality_rating: get_cf_number_f64(entry, sys::kVTVideoEncoderList_QualityRating),
            has_instance_limit: get_cf_bool(entry, sys::kVTVideoEncoderList_InstanceLimit),
        })
    }
}

/// `VTCopyVideoEncoderList` から指定した ID のエンコーダー情報を探す
///
/// 一覧の取得に失敗した場合と、対象コーデックのエントリに ID が一致するものが無かった場合は
/// `None` を返す。
#[cfg(target_os = "macos")]
fn find_encoder_info(codec: VideoCodecType, encoder_id: &str) -> Option<EncodingInfo> {
    let list = encoder_list(codec.to_fourcc())?;
    list.entries.iter().find_map(|&entry| {
        let info = unsafe { encoding_info(entry) }?;
        if info.encoder_id == encoder_id {
            Some(info)
        } else {
            None
        }
    })
}

/// `kVTVideoEncoderList_CodecType` から FourCC を取り出す
///
/// キーが存在しない、`CFNumber` でない、数値として取り出せない場合は `None` を返す。
/// FourCC は `UInt32` だが `CFNumber` からは符号付き 32 bit として読み出し、
/// ビット列をそのまま `u32` として扱う。
#[cfg(target_os = "macos")]
unsafe fn encoder_fourcc(entry: sys::CFDictionaryRef) -> Option<u32> {
    unsafe {
        let value =
            sys::CFDictionaryGetValue(entry, sys::kVTVideoEncoderList_CodecType as *const c_void);
        if value.is_null() || sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFNumberGetTypeID()
        {
            return None;
        }

        let mut fourcc: i32 = 0;
        let ok = sys::CFNumberGetValue(
            value.cast(),
            sys::kCFNumberSInt32Type as sys::CFNumberType,
            (&mut fourcc as *mut i32).cast(),
        );
        if ok == 0 {
            return None;
        }
        Some(fourcc as u32)
    }
}

/// 辞書から CFBoolean の値を取り出す
///
/// キーが存在しない場合と、値が CFBoolean でない場合は `None` を返す。
/// キーが無い場合の既定値はキーごとに異なる（`kVTVideoEncoderList_IsHardwareAccelerated` /
/// `kVTVideoEncoderList_InstanceLimit` / `kVTVideoEncoderList_SupportsMultiPass` は false だが、
/// `kVTVideoEncoderList_SupportsFrameReordering` は true）ため、既定値の適用は呼び出し側で行う。
#[cfg(target_os = "macos")]
unsafe fn get_cf_bool(dict: sys::CFDictionaryRef, key: sys::CFStringRef) -> Option<bool> {
    unsafe {
        let value = sys::CFDictionaryGetValue(dict, key as *const c_void);
        if value.is_null() || sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFBooleanGetTypeID()
        {
            return None;
        }
        Some(sys::CFBooleanGetValue(value.cast()) != 0)
    }
}

/// 辞書から CFString の値を取り出す
///
/// キーが存在しない、CFString でない、UTF-8 に変換できない場合は `None` を返す。
#[cfg(target_os = "macos")]
unsafe fn get_cf_string(dict: sys::CFDictionaryRef, key: sys::CFStringRef) -> Option<String> {
    unsafe {
        let value = sys::CFDictionaryGetValue(dict, key as *const c_void);
        if value.is_null() || sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFStringGetTypeID()
        {
            return None;
        }
        cf_string_to_string(value as sys::CFStringRef)
    }
}

/// 辞書から CFNumber の値を f64 として取り出す
///
/// キーが存在しない、CFNumber でない、f64 に変換できない場合は `None` を返す。
/// Video Toolbox は同じキーでも整数型と浮動小数点型の CFNumber を返しうるため、
/// どちらも表現できる f64 として読む。
#[cfg(target_os = "macos")]
unsafe fn get_cf_number_f64(dict: sys::CFDictionaryRef, key: sys::CFStringRef) -> Option<f64> {
    unsafe {
        let value = sys::CFDictionaryGetValue(dict, key as *const c_void);
        if value.is_null() || sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFNumberGetTypeID()
        {
            return None;
        }
        let mut number = 0.0f64;
        let ok = sys::CFNumberGetValue(
            value.cast(),
            sys::kCFNumberFloat64Type as sys::CFNumberType,
            (&mut number as *mut f64).cast(),
        );
        if ok == 0 {
            return None;
        }
        Some(number)
    }
}

/// CFString を Rust の `String` に変換する
///
/// UTF-8 への変換に失敗した場合は `None` を返す。
#[cfg(target_os = "macos")]
unsafe fn cf_string_to_string(s: sys::CFStringRef) -> Option<String> {
    unsafe {
        let length = sys::CFStringGetLength(s);
        // 終端の NUL を含めた最大バイト数を求める
        let max_size = sys::CFStringGetMaximumSizeForEncoding(length, sys::kCFStringEncodingUTF8);
        if max_size < 0 {
            return None;
        }
        let mut buffer = vec![0u8; max_size as usize + 1];
        let ok = sys::CFStringGetCString(
            s,
            buffer.as_mut_ptr().cast(),
            buffer.len() as sys::CFIndex,
            sys::kCFStringEncodingUTF8,
        );
        if ok == 0 {
            return None;
        }
        // CFStringGetCString は NUL 終端するため、NUL の手前までを String にする
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        String::from_utf8(buffer[..end].to_vec()).ok()
    }
}

/// サポートプロパティ辞書からプロファイル一覧を取り出す
///
/// `properties` は `VTCopySupportedPropertyDictionaryForEncoder` が返した辞書で、
/// 呼び出し元が所有権を保持している必要がある。
///
/// 辞書に `kVTCompressionPropertyKey_ProfileLevel` が無い場合や、その値に
/// `kVTPropertySupportedValueListKey` が無い場合は、プロファイル情報を取得できなかったものとして
/// `None` を返す。
#[cfg(target_os = "macos")]
fn query_encoding_profiles(
    codec: VideoCodecType,
    properties: Option<&CfPtr<c_void>>,
) -> Option<EncodingProfiles> {
    let properties = properties?;
    let props = properties.0 as sys::CFDictionaryRef;
    unsafe {
        if sys::CFGetTypeID(props as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
            return None;
        }

        let profile_entry = sys::CFDictionaryGetValue(
            props,
            sys::kVTCompressionPropertyKey_ProfileLevel as *const c_void,
        );
        if profile_entry.is_null() {
            return None;
        }
        if sys::CFGetTypeID(profile_entry) != sys::CFDictionaryGetTypeID() {
            return None;
        }

        let value_list = sys::CFDictionaryGetValue(
            profile_entry as sys::CFDictionaryRef,
            sys::kVTPropertySupportedValueListKey as *const c_void,
        );
        if value_list.is_null() {
            return None;
        }
        if sys::CFGetTypeID(value_list as sys::CFTypeRef) != sys::CFArrayGetTypeID() {
            return None;
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
                Some(EncodingProfiles::H264(match_profiles(
                    value_array,
                    count,
                    map,
                )))
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
                Some(EncodingProfiles::Hevc(match_profiles(
                    value_array,
                    count,
                    map,
                )))
            }
            // VP9 / AV1 はプロファイルの対応表を持たない
            _ => None,
        }
    }
}

/// 値リストのうち `map` に載っている値を、値リストの順に重複なく集める
///
/// `CFString` でない値と `map` に無い値は無視する。
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

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use super::*;

    /// `kVTVideoEncoderList_CodecType` のキーを返す
    ///
    /// extern static の参照は unsafe なので、テスト内の各所で unsafe ブロックを
    /// 書かなくて済むようにここへまとめる。
    fn codec_type_key() -> sys::CFStringRef {
        unsafe { sys::kVTVideoEncoderList_CodecType }
    }

    /// `kVTVideoEncoderList_SupportsFrameReordering` のキーを返す
    fn frame_reordering_key() -> sys::CFStringRef {
        unsafe { sys::kVTVideoEncoderList_SupportsFrameReordering }
    }

    /// `kVTVideoEncoderList_SupportsFrameReordering` をキーとする辞書を生成する
    ///
    /// `value` に `None` を渡すと、このキーを持たない空の辞書を生成する。
    fn frame_reordering_dictionary(value: Option<*const c_void>) -> CfPtr<c_void> {
        let kvs = match value {
            Some(value) => vec![(frame_reordering_key(), value)],
            None => Vec::new(),
        };
        crate::types::cf_dictionary(&kvs).expect("CFDictionaryCreate に失敗した")
    }

    /// `get_cf_bool` がキーの欠落を `None` で返すことを検証する
    ///
    /// キーが無い場合の既定値はキーごとに異なる（`kVTVideoEncoderList_SupportsFrameReordering`
    /// は true、`kVTVideoEncoderList_SupportsMultiPass` は false）ため、`false` に潰さず
    /// `None` を返して呼び出し側に既定値を決めさせる必要がある。
    /// ここで `false` を返すようになると、フレームリオーダリングの既定値が反転してしまう。
    #[test]
    fn test_get_cf_bool_missing_key() {
        let dict = frame_reordering_dictionary(None);
        let value = unsafe { get_cf_bool(dict.0.cast_mut().cast(), frame_reordering_key()) };
        assert_eq!(
            value, None,
            "キーが存在しない場合は None になること (false に潰してはならない)"
        );
    }

    /// `get_cf_bool` が CFBoolean の true / false をそのまま返すことを検証する
    #[test]
    fn test_get_cf_bool_boolean_value() {
        let cases = unsafe {
            [
                (true, sys::kCFBooleanTrue as *const c_void),
                (false, sys::kCFBooleanFalse as *const c_void),
            ]
        };
        for (expected, raw) in cases {
            let dict = frame_reordering_dictionary(Some(raw));
            let value = unsafe { get_cf_bool(dict.0.cast_mut().cast(), frame_reordering_key()) };
            assert_eq!(
                value,
                Some(expected),
                "CFBoolean の値が {expected} として読み出されること"
            );
        }
    }

    /// `get_cf_bool` が CFBoolean でない値を `None` で返すことを検証する
    ///
    /// `CFDictionaryGetValue` が返す値の型は `CFGetTypeID` で確認する必要がある。
    /// 型を確認せずに `CFBooleanGetValue` へ渡すと、無関係な CF オブジェクトを
    /// CFBoolean として解釈してしまう。
    #[test]
    fn test_get_cf_bool_non_boolean_value() {
        let number = crate::types::cf_number_i32(1).expect("CFNumberCreate に失敗した");
        let dict = frame_reordering_dictionary(Some(number.0));
        let value = unsafe { get_cf_bool(dict.0.cast_mut().cast(), frame_reordering_key()) };
        assert_eq!(
            value, None,
            "CFBoolean でない値は None になること (型を確認せずに解釈してはならない)"
        );
    }

    /// `encoder_fourcc` が `kVTVideoEncoderList_CodecType` から FourCC を取り出すことを検証する
    ///
    /// `VTCopyVideoEncoderList` の結果から対象コーデックのエントリを絞り込む経路で使う。
    /// `CFNumber` を `u32` として読むため、FourCC が `i32` の範囲に収まる全ての値について
    /// 読み出せることをここで固定する。
    #[test]
    fn test_encoder_fourcc() {
        for codec in VideoCodecType::all() {
            let fourcc = codec.to_fourcc();
            let number =
                crate::types::cf_number_i32(fourcc as i32).expect("CFNumberCreate に失敗した");
            let dict = crate::types::cf_dictionary(&[(codec_type_key(), number.0)])
                .expect("CFDictionaryCreate に失敗した");

            let value = unsafe { encoder_fourcc(dict.0.cast_mut().cast()) };
            assert_eq!(value, Some(fourcc), "{codec:?} の FourCC が読み出せない");
        }
    }

    /// `encoder_fourcc` が `kVTVideoEncoderList_CodecType` を持たない辞書と、
    /// このキーの値が CFNumber でない辞書に対して `None` を返すことを検証する
    #[test]
    fn test_encoder_fourcc_missing_or_invalid() {
        // キーが存在しない場合
        let dict = crate::types::cf_dictionary(&[]).expect("CFDictionaryCreate に失敗した");
        let value = unsafe { encoder_fourcc(dict.0.cast_mut().cast()) };
        assert_eq!(value, None, "キーが存在しない場合は None になること");

        // キーは存在するが CFNumber でない場合
        let boolean = unsafe { sys::kCFBooleanTrue as *const c_void };
        let dict = crate::types::cf_dictionary(&[(codec_type_key(), boolean)])
            .expect("CFDictionaryCreate に失敗した");
        let value = unsafe { encoder_fourcc(dict.0.cast_mut().cast()) };
        assert_eq!(value, None, "CFNumber でない値は None になること");
    }
}
