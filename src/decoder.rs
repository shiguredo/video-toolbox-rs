use std::{
    ffi::{c_int, c_void},
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

// デコード完了通知コールバック型
// `Err` の場合はデコード失敗や出力欠落を表す。
type DecodeCallback<T> = dyn FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static;

// `decompressionOutputRefCon` で受け渡すコールバック本体。
// FFI へは `&DecodeCallbackBox<T>` のポインタを渡す。
type DecodeCallbackBox<T> = Box<DecodeCallback<T>>;

// 1 回の decode 呼び出しに対応するデータ保持領域。
// 非同期デコード完了コールバックが来るまで圧縮データと CoreMedia オブジェクトを保持する。
struct PendingDecode<T> {
    user_data: T,
    pixel_format: PixelFormat,
    owned: Vec<u8>,
    block_buffer: CfPtrMut<c_void>,
    sample_buffer: CfPtrMut<c_void>,
}

// Decoder が内部に保持するフォーマット状態。
// `decode()` で SPS/PPS/VPS の変化を検出するため、現在の値を所有する形で保持する。
// パラメータセットを保持しない VP9 / AV1 はサイズのみ保持する。
enum FormatState {
    H264 {
        sps: Vec<u8>,
        pps: Vec<u8>,
        nalu_len_bytes: u32,
    },
    Hevc {
        vps: Vec<u8>,
        sps: Vec<u8>,
        pps: Vec<u8>,
        nalu_len_bytes: u32,
    },
    Vp9 {
        width: u32,
        height: u32,
    },
    Av1 {
        width: u32,
        height: u32,
    },
}

impl FormatState {
    fn from_codec(codec: &DecoderCodec<'_>) -> Self {
        match codec {
            DecoderCodec::H264 {
                sps,
                pps,
                nalu_len_bytes,
            } => Self::H264 {
                sps: sps.to_vec(),
                pps: pps.to_vec(),
                nalu_len_bytes: *nalu_len_bytes,
            },
            DecoderCodec::Hevc {
                vps,
                sps,
                pps,
                nalu_len_bytes,
            } => Self::Hevc {
                vps: vps.to_vec(),
                sps: sps.to_vec(),
                pps: pps.to_vec(),
                nalu_len_bytes: *nalu_len_bytes,
            },
            DecoderCodec::Vp9 { width, height } => Self::Vp9 {
                width: *width,
                height: *height,
            },
            DecoderCodec::Av1 { width, height } => Self::Av1 {
                width: *width,
                height: *height,
            },
        }
    }

    fn as_codec(&self) -> DecoderCodec<'_> {
        match self {
            Self::H264 {
                sps,
                pps,
                nalu_len_bytes,
            } => DecoderCodec::H264 {
                sps,
                pps,
                nalu_len_bytes: *nalu_len_bytes,
            },
            Self::Hevc {
                vps,
                sps,
                pps,
                nalu_len_bytes,
            } => DecoderCodec::Hevc {
                vps,
                sps,
                pps,
                nalu_len_bytes: *nalu_len_bytes,
            },
            Self::Vp9 { width, height } => DecoderCodec::Vp9 {
                width: *width,
                height: *height,
            },
            Self::Av1 { width, height } => DecoderCodec::Av1 {
                width: *width,
                height: *height,
            },
        }
    }
}

/// H.264 / H.265 / VP9 / AV1 デコーダー
///
/// デコード完了時に `FnMut(Result<DecodedFrame<T>, Error>)` を呼び出す。
/// コールバックは Video Toolbox のコールバックスレッドで実行される。
///
/// `decode()` は H.264 / H.265 の入力 (AVCC 形式) を内部で走査し、SPS / PPS / VPS の
/// 変化を検出すると `CMVideoFormatDescription` を自動的に再構築する。
/// VP9 / AV1 は内部にビットストリームパーサーを持たないため、自動検出は行わない。
pub struct Decoder<T: Send + 'static> {
    description: sys::CMVideoFormatDescriptionRef,
    session: sys::VTDecompressionSessionRef,
    pixel_format: PixelFormat,
    format_state: FormatState,
    callback: Box<DecodeCallbackBox<T>>,
}

impl<T: Send + 'static> Decoder<T> {
    /// デコーダーのインスタンスを生成する
    pub fn new<F>(config: DecoderConfig<'_>, on_decoded: F) -> Result<Self, Error>
    where
        F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static,
    {
        // `DecodeCallback<T>` は fat pointer であるため、さらに `Box` でラップして通常のポインタにする。
        let callback = Box::new(Box::new(on_decoded) as DecodeCallbackBox<T>);

        unsafe {
            let description = Self::create_format_description(&config.codec)
                .map_err(|e| Self::wrap_unsupported_codec_error(&config.codec, e))?;
            let session = Self::create_decompression_session(
                description,
                config.pixel_format,
                callback.as_ref(),
            )
            .map_err(|e| {
                sys::CFRelease(description as *const c_void);
                Self::wrap_unsupported_codec_error(&config.codec, e)
            })?;

            let format_state = FormatState::from_codec(&config.codec);

            Ok(Self {
                description,
                session,
                pixel_format: config.pixel_format,
                format_state,
                callback,
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

    /// 検出した新しい `FormatState` をデコーダーに適用する
    ///
    /// `VTDecompressionSessionCanAcceptFormatDescription` で既存セッションが新しい
    /// `CMVideoFormatDescription` を受け入れ可能か判定する。
    /// 可能なら description のみ差し替え、不可能ならセッションを再作成する。
    /// 再作成前には未出力フレームを `finish()` でフラッシュする。
    fn apply_format_change(&mut self, new_state: FormatState) -> Result<(), Error> {
        self.finish()?;

        unsafe {
            let new_description = Self::create_format_description(&new_state.as_codec())?;

            let can_accept = sys::VTDecompressionSessionCanAcceptFormatDescription(
                self.session,
                new_description,
            );

            if can_accept != 0 {
                // 受け入れ可能: description のみ差し替える
                sys::CFRelease(self.description as *const c_void);
                self.description = new_description;
            } else {
                // 受け入れ不可能: セッションを再作成する
                // 新しいセッションを先に作成し、失敗時に self が不整合にならないようにする
                let new_session = match Self::create_decompression_session(
                    new_description,
                    self.pixel_format,
                    self.callback.as_ref(),
                ) {
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

            self.format_state = new_state;
            Ok(())
        }
    }

    /// これ以上データが来ないことをデコーダーに伝える
    ///
    /// 遅延フレームを排出し、非同期コールバック完了まで待機する。
    pub fn finish(&mut self) -> Result<(), Error> {
        unsafe {
            let status = sys::VTDecompressionSessionFinishDelayedFrames(self.session);
            Error::check(status, "VTDecompressionSessionFinishDelayedFrames")?;

            let status = sys::VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            Error::check(status, "VTDecompressionSessionWaitForAsynchronousFrames")?;
        }
        Ok(())
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
        callback_ref_con: &DecodeCallbackBox<T>,
    ) -> Result<sys::VTDecompressionSessionRef, Error> {
        unsafe {
            let mut session: sys::VTDecompressionSessionRef = std::ptr::null_mut();
            // 現行 SDK では `VTDecompressionOutputCallbackRecord` はコールバック関数ポインタと refcon の 2 フィールドのみ。
            // ゼロ初期化で refcon は NULL。続けてコールバックと refcon を代入する方針である（issue 0025）。
            let mut callback =
                MaybeUninit::<sys::VTDecompressionOutputCallbackRecord>::zeroed().assume_init();
            callback.decompressionOutputCallback = Some(Self::output_callback);
            callback.decompressionOutputRefCon = (callback_ref_con as *const DecodeCallbackBox<T>)
                .cast::<c_void>()
                .cast_mut();

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
    /// `user_data` は対応するデコード完了時に `DecodedFrame<T>` に載せて返す。
    /// 完了通知は `Decoder::new` で渡したコールバックで受け取る。
    ///
    /// H.264 / H.265 の入力 (AVCC 形式) は内部で走査され、SPS / PPS / VPS が前回と異なる
    /// 場合は自動的に `CMVideoFormatDescription` を再構築する。VP9 / AV1 は
    /// 自動検出を行わない (内部にビットストリームパーサーを持たないため)。
    /// 自動検出に伴うパース失敗はログ出力のみで、既存セッションでデコードを続行する
    /// (フェイルオープン)。新しいパラメータセットでのフォーマット適用に失敗した場合は
    /// このメソッドの戻り値で `Err` を返す。
    pub fn decode(&mut self, data: &[u8], user_data: T) -> Result<(), Error> {
        // ストリームから SPS/PPS/VPS の変化を検出し、必要なら内部でフォーマットを更新する。
        self.auto_detect_format_change(data)?;

        // `CMBlockBufferCreateWithMemoryBlock` に渡すメモリを `Vec` で所有する。
        // `&[u8]` からミュータブルポインタを渡すとエイリアス規則上の未定義動作の余地があるため、
        // コピーで所有権を明確にする（CoreMedia はデコード時に参照するのみ）。
        let owned = data.to_vec();

        unsafe {
            let mut block_buffer_ref = std::ptr::null_mut();
            let status = sys::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null_mut(),
                owned.as_ptr().cast_mut().cast(),
                owned.len(),
                sys::kCFAllocatorNull, // data の自動解放を Video Toolbox 側で行わないようにする
                std::ptr::null(),
                0,
                owned.len(),
                0,
                &mut block_buffer_ref,
            );
            Error::check(status, "CMBlockBufferCreateWithMemoryBlock")?;
            let block_buffer = CfPtrMut(block_buffer_ref.cast::<c_void>());

            let mut sample_buffer_ref = std::ptr::null_mut();
            let status = sys::CMSampleBufferCreateReady(
                std::ptr::null_mut(),
                block_buffer.0.cast(),
                self.description,
                1,
                0,
                [].as_ptr(),
                0,
                [].as_ptr(),
                &mut sample_buffer_ref,
            );
            Error::check(status, "CMSampleBufferCreateReady")?;
            let sample_buffer = CfPtrMut(sample_buffer_ref.cast::<c_void>());

            let pending = Box::new(PendingDecode {
                user_data,
                pixel_format: self.pixel_format,
                owned,
                block_buffer,
                sample_buffer,
            });
            let source_frame_ref_con = Box::into_raw(pending).cast::<c_void>();

            let decode_flags = sys::kVTDecodeFrame_EnableAsynchronousDecompression;
            let mut info_flags = 0;
            let status = sys::VTDecompressionSessionDecodeFrame(
                self.session,
                sample_buffer_ref,
                decode_flags,
                source_frame_ref_con,
                &mut info_flags,
            );
            if let Err(e) = Error::check(status, "VTDecompressionSessionDecodeFrame") {
                let _ = Box::from_raw(source_frame_ref_con.cast::<PendingDecode<T>>());
                return Err(e);
            }

            Ok(())
        }
    }

    /// AVCC ストリームを走査し、SPS / PPS / VPS の変化を検出した場合に
    /// 内部でフォーマットを更新する。
    ///
    /// 対応コーデックは H.264 / H.265。VP9 / AV1 は何もしない。
    /// パース失敗 (長さフィールド異常等) は `log::warn!` を出して既存設定のまま続行する。
    /// 新しいパラメータセットによるフォーマット適用に失敗した場合は `Err` を返す。
    fn auto_detect_format_change(&mut self, data: &[u8]) -> Result<(), Error> {
        let new_state = match &self.format_state {
            FormatState::H264 {
                sps,
                pps,
                nalu_len_bytes,
            } => detect_h264_change(data, *nalu_len_bytes, sps, pps),
            FormatState::Hevc {
                vps,
                sps,
                pps,
                nalu_len_bytes,
            } => detect_hevc_change(data, *nalu_len_bytes, vps, sps, pps),
            FormatState::Vp9 { .. } | FormatState::Av1 { .. } => None,
        };

        if let Some(new_state) = new_state {
            self.apply_format_change(new_state)?;
        }
        Ok(())
    }

    unsafe fn take_pending_decode(
        source_frame_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Option<Box<PendingDecode<T>>> {
        if source_frame_ref_con.is_null() {
            log::error!("{callback_name}: source_frame_ref_con is null");
            return None;
        }
        Some(unsafe { Box::from_raw(source_frame_ref_con.cast::<PendingDecode<T>>()) })
    }

    unsafe fn callback_from_ref_con<'a>(
        output_callback_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Option<&'a mut DecodeCallbackBox<T>> {
        if output_callback_ref_con.is_null() {
            log::error!("{callback_name}: output_callback_ref_con is null");
            return None;
        }
        Some(unsafe { &mut *output_callback_ref_con.cast::<DecodeCallbackBox<T>>() })
    }

    fn invoke_callback(
        callback: &mut DecodeCallbackBox<T>,
        result: Result<DecodedFrame<T>, Error>,
    ) {
        let callback = callback.as_mut();
        (callback)(result);
    }

    unsafe extern "C" fn output_callback(
        decompression_output_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTDecodeInfoFlags,
        image_buffer: sys::CVImageBufferRef,
        _presentation_time_stamp: sys::CMTime,
        _presentation_duration: sys::CMTime,
    ) {
        let callback_name = "output_callback";
        let Some(pending) =
            (unsafe { Self::take_pending_decode(source_frame_ref_con, callback_name) })
        else {
            return;
        };
        let Some(callback) =
            (unsafe { Self::callback_from_ref_con(decompression_output_ref_con, callback_name) })
        else {
            return;
        };

        let PendingDecode {
            user_data,
            pixel_format,
            owned: _owned,
            block_buffer: _block_buffer,
            sample_buffer: _sample_buffer,
        } = *pending;

        if let Err(e) = Error::check(status, callback_name) {
            log::error!("{e}");
            Self::invoke_callback(callback, Err(e));
            return;
        }

        // フレームドロップ等で image_buffer が NULL になる場合がある
        if image_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "decoded image buffer is null",
            };
            log::error!("{e}");
            Self::invoke_callback(callback, Err(e));
            return;
        }

        // コールバック引数の image_buffer を利用者コールバックの外でも保持できるように retain する。
        let retained_image_buffer = unsafe { sys::CFRetain(image_buffer.cast()) };
        let retained_image_buffer = retained_image_buffer.cast_mut().cast();
        let image_buffer = CfPtrMut(retained_image_buffer);

        let flags_readonly = 1;
        let status = unsafe { sys::CVPixelBufferLockBaseAddress(image_buffer.0, flags_readonly) };
        if let Err(e) = Error::check(status, "CVPixelBufferLockBaseAddress") {
            log::error!("{e}");
            Self::invoke_callback(callback, Err(e));
            return;
        }

        let frame = match pixel_format {
            PixelFormat::I420 => DecodedFrame::I420 {
                frame: I420Frame {
                    inner: image_buffer,
                },
                user_data,
            },
            PixelFormat::Nv12 => DecodedFrame::Nv12 {
                frame: Nv12Frame {
                    inner: image_buffer,
                },
                user_data,
            },
        };
        Self::invoke_callback(callback, Ok(frame));
    }
}

impl<T: Send + 'static> Drop for Decoder<T> {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            log::error!("{e}");
        }

        unsafe {
            sys::VTDecompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);
            sys::CFRelease(self.description as *const c_void);
        }
    }
}

// SAFETY: VTDecompressionSession は内部でスレッドセーフに管理されており、
// Apple のドキュメントでもセッションの操作は異なるスレッドから呼び出し可能とされている。
// コールバックは別スレッドで実行されるため、`T: Send` 制約を課して所有権を安全に移動させる。
unsafe impl<T: Send + 'static> Send for Decoder<T> {}

// AVCC ストリームから NAL ユニットを順に取り出すイテレータ。
// `nalu_len_bytes` が 1 / 2 / 4 以外、長さフィールドが 0、または残りデータを超える場合は
// その時点でイテレーションを終了する (フェイルオープン)。
struct AvccNaluIter<'a> {
    data: &'a [u8],
    nalu_len_bytes: usize,
}

impl<'a> AvccNaluIter<'a> {
    fn new(data: &'a [u8], nalu_len_bytes: u32) -> Option<Self> {
        let nalu_len_bytes = match nalu_len_bytes {
            1 | 2 | 4 => nalu_len_bytes as usize,
            _ => return None,
        };
        Some(Self {
            data,
            nalu_len_bytes,
        })
    }
}

impl<'a> Iterator for AvccNaluIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.len() < self.nalu_len_bytes {
            return None;
        }
        let (len_bytes, rest) = self.data.split_at(self.nalu_len_bytes);
        let nalu_len = match self.nalu_len_bytes {
            1 => len_bytes[0] as usize,
            2 => u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize,
            4 => u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]])
                as usize,
            _ => return None,
        };
        if nalu_len == 0 || nalu_len > rest.len() {
            // 長さ 0 や残りバイト超過は不正。ここで停止する。
            return None;
        }
        let (nalu, next) = rest.split_at(nalu_len);
        self.data = next;
        Some(nalu)
    }
}

// H.264 の NAL ユニットタイプを返す。NAL ユニットが空の場合は None。
// nal_unit_type は NAL ヘッダの下位 5 ビット。
fn h264_nal_unit_type(nalu: &[u8]) -> Option<u8> {
    nalu.first().map(|b| b & 0x1F)
}

// H.265 の NAL ユニットタイプを返す。NAL ユニットが空の場合は None。
// nal_unit_type は NAL ヘッダ先頭バイトの bit 1..7 (`(byte >> 1) & 0x3F`)。
fn h265_nal_unit_type(nalu: &[u8]) -> Option<u8> {
    nalu.first().map(|b| (b >> 1) & 0x3F)
}

// AVCC ストリーム内で最後に出現した H.264 SPS / PPS を抽出する。
// 戻り値は (sps, pps) で、見つからないものは None。
fn extract_h264_param_sets(
    data: &[u8],
    nalu_len_bytes: u32,
) -> (Option<&[u8]>, Option<&[u8]>) {
    let mut last_sps: Option<&[u8]> = None;
    let mut last_pps: Option<&[u8]> = None;
    let Some(iter) = AvccNaluIter::new(data, nalu_len_bytes) else {
        return (None, None);
    };
    for nalu in iter {
        match h264_nal_unit_type(nalu) {
            // SPS
            Some(7) => last_sps = Some(nalu),
            // PPS
            Some(8) => last_pps = Some(nalu),
            _ => {}
        }
    }
    (last_sps, last_pps)
}

// AVCC ストリーム内で最後に出現した H.265 VPS / SPS / PPS を抽出する。
#[allow(clippy::type_complexity)]
fn extract_h265_param_sets(
    data: &[u8],
    nalu_len_bytes: u32,
) -> (Option<&[u8]>, Option<&[u8]>, Option<&[u8]>) {
    let mut last_vps: Option<&[u8]> = None;
    let mut last_sps: Option<&[u8]> = None;
    let mut last_pps: Option<&[u8]> = None;
    let Some(iter) = AvccNaluIter::new(data, nalu_len_bytes) else {
        return (None, None, None);
    };
    for nalu in iter {
        match h265_nal_unit_type(nalu) {
            // VPS_NUT
            Some(32) => last_vps = Some(nalu),
            // SPS_NUT
            Some(33) => last_sps = Some(nalu),
            // PPS_NUT
            Some(34) => last_pps = Some(nalu),
            _ => {}
        }
    }
    (last_vps, last_sps, last_pps)
}

// 入力ストリームに含まれる H.264 SPS / PPS が現在保持中のものと異なる場合のみ
// 新しい `FormatState` を返す。SPS / PPS のいずれかしか含まれない場合は、
// 含まれている方だけを更新候補とし、不在側は現在の値を引き継ぐ。
fn detect_h264_change(
    data: &[u8],
    nalu_len_bytes: u32,
    current_sps: &[u8],
    current_pps: &[u8],
) -> Option<FormatState> {
    let (new_sps, new_pps) = extract_h264_param_sets(data, nalu_len_bytes);
    let sps_changed = matches!(new_sps, Some(s) if s != current_sps);
    let pps_changed = matches!(new_pps, Some(p) if p != current_pps);
    if !sps_changed && !pps_changed {
        return None;
    }
    let sps = new_sps.unwrap_or(current_sps).to_vec();
    let pps = new_pps.unwrap_or(current_pps).to_vec();
    log::info!("H.264 parameter set change detected, applying new format");
    Some(FormatState::H264 {
        sps,
        pps,
        nalu_len_bytes,
    })
}

// 入力ストリームに含まれる H.265 VPS / SPS / PPS が現在保持中のものと異なる場合のみ
// 新しい `FormatState` を返す。
fn detect_hevc_change(
    data: &[u8],
    nalu_len_bytes: u32,
    current_vps: &[u8],
    current_sps: &[u8],
    current_pps: &[u8],
) -> Option<FormatState> {
    let (new_vps, new_sps, new_pps) = extract_h265_param_sets(data, nalu_len_bytes);
    let vps_changed = matches!(new_vps, Some(v) if v != current_vps);
    let sps_changed = matches!(new_sps, Some(s) if s != current_sps);
    let pps_changed = matches!(new_pps, Some(p) if p != current_pps);
    if !vps_changed && !sps_changed && !pps_changed {
        return None;
    }
    let vps = new_vps.unwrap_or(current_vps).to_vec();
    let sps = new_sps.unwrap_or(current_sps).to_vec();
    let pps = new_pps.unwrap_or(current_pps).to_vec();
    log::info!("H.265 parameter set change detected, applying new format");
    Some(FormatState::Hevc {
        vps,
        sps,
        pps,
        nalu_len_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_nalu_len4(nalus: &[&[u8]]) -> Vec<u8> {
        let mut data = Vec::new();
        for nalu in nalus {
            data.extend_from_slice(&(nalu.len() as u32).to_be_bytes());
            data.extend_from_slice(nalu);
        }
        data
    }

    #[test]
    fn iter_skips_unsupported_nalu_len_bytes() {
        // nalu_len_bytes が 1/2/4 以外なら None を返す
        assert!(AvccNaluIter::new(&[0u8; 8], 3).is_none());
        assert!(AvccNaluIter::new(&[0u8; 8], 0).is_none());
        assert!(AvccNaluIter::new(&[0u8; 8], 8).is_none());
    }

    #[test]
    fn iter_stops_on_zero_length() {
        // 長さフィールドが 0 ならその時点で停止
        let data = vec![0u8, 0, 0, 0];
        let collected: Vec<&[u8]> = AvccNaluIter::new(&data, 4).unwrap().collect();
        assert!(collected.is_empty());
    }

    #[test]
    fn iter_stops_on_truncated_length() {
        // 長さフィールドより残りデータが少ない場合は停止
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes());
        data.extend_from_slice(&[1u8, 2, 3]);
        let collected: Vec<&[u8]> = AvccNaluIter::new(&data, 4).unwrap().collect();
        assert!(collected.is_empty());
    }

    #[test]
    fn iter_walks_all_nalus_len4() {
        // 4 バイト長プレフィックスで複数 NAL ユニットを取り出せる
        let data = pack_nalu_len4(&[&[0x01, 0x02, 0x03], &[0xAA, 0xBB]]);
        let collected: Vec<&[u8]> = AvccNaluIter::new(&data, 4).unwrap().collect();
        assert_eq!(collected, vec![&[0x01, 0x02, 0x03][..], &[0xAA, 0xBB][..]]);
    }

    #[test]
    fn detect_h264_change_no_change_returns_none() {
        let sps = vec![0x67, 0x42, 0x00, 0x1E];
        let pps = vec![0x68, 0xCE, 0x3C, 0x80];
        // 同じ SPS/PPS が含まれるストリーム
        let data = pack_nalu_len4(&[&sps, &pps, &[0x65, 0x88, 0x84]]);
        assert!(detect_h264_change(&data, 4, &sps, &pps).is_none());
    }

    #[test]
    fn detect_h264_change_detects_sps_change() {
        let old_sps = vec![0x67, 0x42, 0x00, 0x1E];
        let new_sps = vec![0x67, 0x64, 0x00, 0x1F];
        let pps = vec![0x68, 0xCE, 0x3C, 0x80];
        let data = pack_nalu_len4(&[&new_sps, &pps]);
        let state = detect_h264_change(&data, 4, &old_sps, &pps).expect("change expected");
        match state {
            FormatState::H264 {
                sps,
                pps: pps_out,
                nalu_len_bytes,
            } => {
                assert_eq!(sps, new_sps);
                assert_eq!(pps_out, pps);
                assert_eq!(nalu_len_bytes, 4);
            }
            _ => panic!("expected H264 format state"),
        }
    }

    #[test]
    fn detect_h264_change_pps_only_keeps_old_sps() {
        // 入力に PPS のみが含まれる場合、SPS は現状維持
        let sps = vec![0x67, 0x42, 0x00, 0x1E];
        let old_pps = vec![0x68, 0xCE, 0x3C, 0x80];
        let new_pps = vec![0x68, 0xEB, 0xE3, 0xCB];
        let data = pack_nalu_len4(&[&new_pps]);
        let state = detect_h264_change(&data, 4, &sps, &old_pps).expect("change expected");
        match state {
            FormatState::H264 {
                sps: sps_out,
                pps,
                ..
            } => {
                assert_eq!(sps_out, sps);
                assert_eq!(pps, new_pps);
            }
            _ => panic!("expected H264 format state"),
        }
    }

    #[test]
    fn detect_h264_change_ignores_non_sps_pps_nalus() {
        let sps = vec![0x67, 0x42, 0x00, 0x1E];
        let pps = vec![0x68, 0xCE, 0x3C, 0x80];
        // IDR スライスのみのストリーム
        let data = pack_nalu_len4(&[&[0x65, 0x88, 0x84]]);
        assert!(detect_h264_change(&data, 4, &sps, &pps).is_none());
    }

    #[test]
    fn detect_h264_change_malformed_stream_returns_none() {
        // 長さフィールドより小さいバイト列。フェイルオープンで None を返す。
        let sps = vec![0x67, 0x42, 0x00, 0x1E];
        let pps = vec![0x68, 0xCE, 0x3C, 0x80];
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes());
        data.extend_from_slice(&[0x01, 0x02]);
        assert!(detect_h264_change(&data, 4, &sps, &pps).is_none());
    }

    #[test]
    fn detect_hevc_change_detects_vps_change() {
        let old_vps = vec![0x40, 0x01, 0x0C];
        let new_vps = vec![0x40, 0x01, 0x0D];
        let sps = vec![0x42, 0x01, 0x01];
        let pps = vec![0x44, 0x01, 0xC0];
        let data = pack_nalu_len4(&[&new_vps, &sps, &pps]);
        let state = detect_hevc_change(&data, 4, &old_vps, &sps, &pps).expect("change expected");
        match state {
            FormatState::Hevc { vps, .. } => {
                assert_eq!(vps, new_vps);
            }
            _ => panic!("expected Hevc format state"),
        }
    }

    #[test]
    fn detect_hevc_change_no_change_returns_none() {
        let vps = vec![0x40, 0x01, 0x0C];
        let sps = vec![0x42, 0x01, 0x01];
        let pps = vec![0x44, 0x01, 0xC0];
        let data = pack_nalu_len4(&[&vps, &sps, &pps]);
        assert!(detect_hevc_change(&data, 4, &vps, &sps, &pps).is_none());
    }
}

/// デコードされた映像フレーム
pub enum DecodedFrame<T> {
    /// I420 形式
    I420 {
        /// デコード済みフレーム本体
        frame: I420Frame,
        /// 入力時に指定したユーザーデータ
        user_data: T,
    },
    /// NV12 形式
    Nv12 {
        /// デコード済みフレーム本体
        frame: Nv12Frame,
        /// 入力時に指定したユーザーデータ
        user_data: T,
    },
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
pub struct I420Frame {
    inner: CfPtrMut<sys::__CVBuffer>,
}

impl I420Frame {
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

impl Drop for I420Frame {
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
pub struct Nv12Frame {
    inner: CfPtrMut<sys::__CVBuffer>,
}

impl Nv12Frame {
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

impl Drop for Nv12Frame {
    fn drop(&mut self) {
        unsafe {
            let flags_readonly = 1;
            sys::CVPixelBufferUnlockBaseAddress(self.inner.0, flags_readonly);
        }
    }
}
