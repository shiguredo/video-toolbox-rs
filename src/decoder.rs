use std::{
    ffi::{c_int, c_void},
    marker::PhantomData,
    mem::MaybeUninit,
};

use crate::{
    error::Error,
    sys,
    types::{
        CfPtr, CfPtrMut, PixelFormat, cf_dictionary, cf_number_i32,
        validate_video_dimensions_for_toolbox,
    },
};

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

    /// VP9/AV1 コーデックの場合、Video Toolbox の失敗のみ `UnsupportedCodec` に変換する（設定不整合の `InvalidConfig` はそのまま返す）。
    fn wrap_unsupported_codec_error(codec: &DecoderCodec<'_>, error: Error) -> Error {
        match codec {
            DecoderCodec::Vp9 { .. } => match error {
                Error::VideoToolbox { .. } => Error::UnsupportedCodec { codec: "VP9" },
                _ => error,
            },
            DecoderCodec::Av1 { .. } => match error {
                Error::VideoToolbox { .. } => Error::UnsupportedCodec { codec: "AV1" },
                _ => error,
            },
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
                    validate_video_dimensions_for_toolbox(*width, *height)?;
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
                    validate_video_dimensions_for_toolbox(*width, *height)?;
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
            // 現行 SDK では `VTDecompressionOutputCallbackRecord` はコールバック関数ポインタと refcon の 2 フィールドのみ。
            // ゼロ初期化で refcon は NULL。続けてコールバックのみ代入する方針である（issue 0025）。
            let mut callback =
                MaybeUninit::<sys::VTDecompressionOutputCallbackRecord>::zeroed().assume_init();
            callback.decompressionOutputCallback = Some(Self::output_callback);

            let cv_pixel_format = match pixel_format {
                PixelFormat::I420 => sys::kCVPixelFormatType_420YpCbCr8Planar,
                PixelFormat::Nv12 => sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            };
            let pf = cf_number_i32(cv_pixel_format as i32)?;
            let dest_attrs = cf_dictionary(&[(sys::kCVPixelBufferPixelFormatTypeKey, pf.0)])?;
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
    ///
    /// `owned`（圧縮データの `Vec`）は `CMBlockBufferCreateWithMemoryBlock` が参照する。
    /// `VTDecompressionSessionDecodeFrame` はこの関数内で同期的に完了するため、
    /// ブロックバッファとサンプルバッファが解放される前にピクセルバッファの内容が確定する。
    /// `kCFAllocatorNull` により `Vec` のヒープ領域は CoreMedia 側で解放されない。
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedFrame<'_>>, Error> {
        // `CMBlockBufferCreateWithMemoryBlock` に渡すメモリを `Vec` で所有する。
        // `&[u8]` からミュータブルポインタを渡すとエイリアス規則上の未定義動作の余地があるため、
        // コピーで所有権を明確にする（CoreMedia はデコード時に参照するのみ）。
        let owned = data.to_vec();
        unsafe {
            let mut block_buffer = std::ptr::null_mut();
            let status = sys::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null_mut(),
                owned.as_ptr().cast_mut().cast(),
                owned.len(),
                sys::kCFAllocatorNull, // data の自動解放を Video Toolbox 側で行わないようにする
                std::ptr::null(),
                0,
                owned.len(),
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

        // フレームドロップ等で image_buffer が NULL になる場合がある
        if image_buffer.is_null() {
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
///
/// [`Decoder::decode`] が `Some` を返しても、プレーン参照が空スライスになることは **あり得る**（[`I420Frame`] / [`Nv12Frame`] の各 `*_plane` 参照）。
pub enum DecodedFrame<'a> {
    /// I420 形式
    I420(I420Frame<'a>),
    /// NV12 形式
    Nv12(Nv12Frame<'a>),
}

/// I420 形式のデコード済みフレーム (3 プレーン: Y, U, V)
///
/// ## プレーン参照
///
/// プラットフォームが基底アドレスに NULL を返した場合、または行数とストライドの乗算が
/// `usize` で表現できない場合、各 `*_plane` は **空のスライス**を返す。
/// 解像度が正である通常のデコードでは空にはならない想定である。
///
/// **空スライスは「デコードが成功したがピクセルが無い」ではなく、異常時のセンチネル**として扱う。
/// 呼び出し側は `y_plane().is_empty()` 等で分岐し、通常のピクセル処理に進まないこと。
#[derive(Debug)]
pub struct I420Frame<'a> {
    inner: CfPtrMut<sys::__CVBuffer>,

    // inner の中には Video Toolbox が返した一時的なデータへの参照も含まれているので、
    // このライフタイムで利用側での使用範囲を制限する。
    _lifetime: PhantomData<&'a ()>,
}

impl I420Frame<'_> {
    /// ロック済みプレーンを `&[u8]` として返す
    ///
    /// NULL または乗算オーバーフロー時は空スライス（[`I420Frame`] の説明を参照）。**型は `Result` ではなく**、空で異常を表す。
    fn plane_slice(&self, plane_index: usize, row_count: usize, bytes_per_row: usize) -> &[u8] {
        let len = match row_count.checked_mul(bytes_per_row) {
            Some(n) if n > 0 => n,
            _ => return &[],
        };
        let ptr = unsafe {
            sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, plane_index) as *const u8
        };
        if ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        self.plane_slice(0, self.height(), self.y_stride())
    }

    /// フレームの U 成分のデータを返す
    pub fn u_plane(&self) -> &[u8] {
        self.plane_slice(1, self.height().div_ceil(2), self.u_stride())
    }

    /// フレームの V 成分のデータを返す
    pub fn v_plane(&self) -> &[u8] {
        self.plane_slice(2, self.height().div_ceil(2), self.v_stride())
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
///
/// ## プレーン参照
///
/// [`I420Frame`] と同様、異常時は各 `*_plane` が空スライスになることがある。
///
/// **空スライスは異常時のセンチネル**であり、空でないことを前提にピクセル処理して進めないこと。
#[derive(Debug)]
pub struct Nv12Frame<'a> {
    inner: CfPtrMut<sys::__CVBuffer>,

    // inner の中には Video Toolbox が返した一時的なデータへの参照も含まれているので、
    // このライフタイムで利用側での使用範囲を制限する。
    _lifetime: PhantomData<&'a ()>,
}

impl Nv12Frame<'_> {
    /// ロック済みプレーンを `&[u8]` として返す
    ///
    /// NULL または乗算オーバーフロー時は空スライス（[`Nv12Frame`] の説明を参照）。**型は `Result` ではなく**、空で異常を表す。
    fn plane_slice(&self, plane_index: usize, row_count: usize, bytes_per_row: usize) -> &[u8] {
        let len = match row_count.checked_mul(bytes_per_row) {
            Some(n) if n > 0 => n,
            _ => return &[],
        };
        let ptr = unsafe {
            sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, plane_index) as *const u8
        };
        if ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        self.plane_slice(0, self.height(), self.y_stride())
    }

    /// フレームの UV インターリーブデータを返す
    pub fn uv_plane(&self) -> &[u8] {
        self.plane_slice(1, self.height().div_ceil(2), self.uv_stride())
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
