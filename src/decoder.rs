use std::ffi::{c_int, c_void};

use crate::{
    error::Error,
    sys::{self, OpaqueCMBlockBuffer, opaqueCMSampleBuffer},
    types::{
        CfPtrMut, PixelFormat, cf_dictionary, cf_number_i32, validate_video_dimensions_for_toolbox,
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

/// デコード結果を通知するためのハンドラー
///
/// デコード処理が完了するたびに [`DecodeHandler::on_decoded`] が呼ばれる。
/// `on_decoded` 内で panic してもプロセスは abort せず、panic は捕捉されて
/// エラーログ（コールバック名 + panic メッセージ）が出力され、デコードセッションは継続する。
/// ただし、ホストアプリが abort するカスタム panic hook をインストールしている場合や
/// `panic=abort` ビルドでは捕捉されず abort する。
pub trait DecodeHandler: Send + 'static {
    /// ユーザーデータ型
    type UserData: Send + 'static;
    /// エラー型
    type Error: From<crate::Error> + Send + 'static;
    /// デコード完了時に呼ばれる
    fn on_decoded(&mut self, result: Result<DecodedFrame<Self::UserData>, Self::Error>);
}

/// `FnMut(Result<DecodedFrame<T>, E>)` を [`DecodeHandler`] にするラッパー
pub struct FnDecodeHandler<T, E = crate::Error> {
    f: Box<dyn FnMut(Result<DecodedFrame<T>, E>) + Send + 'static>,
}

impl<T, E> FnDecodeHandler<T, E> {
    /// `FnMut(Result<DecodedFrame<T>, E>)` から [`DecodeHandler`] を構築する
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(Result<DecodedFrame<T>, E>) + Send + 'static,
    {
        Self { f: Box::new(f) }
    }
}

impl<T, E> DecodeHandler for FnDecodeHandler<T, E>
where
    T: Send + 'static,
    E: From<crate::Error> + Send + 'static,
{
    type UserData = T;
    type Error = E;
    fn on_decoded(&mut self, result: Result<DecodedFrame<T>, E>) {
        (self.f)(result);
    }
}

// 1 回の decode 呼び出しに対応するデータ保持領域。
// 非同期デコード完了コールバックが来るまで圧縮データと CoreMedia オブジェクトを保持する。
struct PendingDecode<T> {
    user_data: T,
    pixel_format: PixelFormat,
    // 以下はデコード中の寿命が切れないようにするために必要
    #[expect(dead_code)]
    owned: Vec<u8>,
    #[expect(dead_code)]
    block_buffer: CfPtrMut<OpaqueCMBlockBuffer>,
    #[expect(dead_code)]
    sample_buffer: CfPtrMut<opaqueCMSampleBuffer>,
}

/// H.264 / H.265 / VP9 / AV1 デコーダー
///
/// デコード完了時に [`DecodeHandler::on_decoded`] を呼び出す。
/// この [`DecodeHandler::on_decoded`] の呼び出しは Video Toolbox のコールバックスレッドから行われる。
pub struct Decoder<H: DecodeHandler> {
    description: sys::CMVideoFormatDescriptionRef,
    session: sys::VTDecompressionSessionRef,
    pixel_format: PixelFormat,
    handler: Box<H>,
}

impl<H: DecodeHandler> Decoder<H> {
    /// デコーダーのインスタンスを生成する
    pub fn new(config: DecoderConfig<'_>, handler: H) -> Result<Self, Error> {
        let handler = Box::new(handler);

        unsafe {
            let description = Self::create_format_description(&config.codec)
                .map_err(|e| Self::wrap_unsupported_codec_error(&config.codec, e))?;
            let session = Self::create_decompression_session(
                description,
                config.pixel_format,
                handler.as_ref(),
            )
            .map_err(|e| {
                sys::CFRelease(description as *const c_void);
                Self::wrap_unsupported_codec_error(&config.codec, e)
            })?;

            Ok(Self {
                description,
                session,
                pixel_format: config.pixel_format,
                handler,
            })
        }
    }

    /// VP9/AV1 コーデックの場合、Video Toolbox の失敗のみ `UnsupportedCodec` に変換する（設定不整合の `InvalidConfig` はそのまま返す）。
    fn wrap_unsupported_codec_error(codec: &DecoderCodec<'_>, error: Error) -> Error {
        match codec {
            DecoderCodec::Vp9 { .. } => match error {
                Error::VideoToolbox { .. } => Error::UnsupportedCodec {
                    codec: "VP9".into(),
                },
                _ => error,
            },
            DecoderCodec::Av1 { .. } => match error {
                Error::VideoToolbox { .. } => Error::UnsupportedCodec {
                    codec: "AV1".into(),
                },
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
        self.finish()?;

        unsafe {
            let new_description = Self::create_format_description(&codec)
                .map_err(|e| Self::wrap_unsupported_codec_error(&codec, e))?;

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
                let new_session = match Self::create_decompression_session(
                    new_description,
                    self.pixel_format,
                    self.handler.as_ref(),
                ) {
                    Ok(session) => session,
                    Err(e) => {
                        sys::CFRelease(new_description as *const c_void);
                        return Err(Self::wrap_unsupported_codec_error(&codec, e));
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
                    // Apple のドキュメントで有効値は 1, 2, 4 のみ
                    if !matches!(nalu_len_bytes, 1 | 2 | 4) {
                        return Err(Error::InvalidConfig {
                            field: "nalu_len_bytes".into(),
                            reason: "must be 1, 2, or 4".into(),
                        });
                    }
                    // 空スライスの as_ptr() は dangling pointer になるため拒否する
                    if sps.is_empty() || pps.is_empty() {
                        return Err(Error::InvalidConfig {
                            field: "parameter_sets".into(),
                            reason: "sps and pps must not be empty".into(),
                        });
                    }
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
                    // Apple のドキュメントで有効値は 1, 2, 4 のみ
                    if !matches!(nalu_len_bytes, 1 | 2 | 4) {
                        return Err(Error::InvalidConfig {
                            field: "nalu_len_bytes".into(),
                            reason: "must be 1, 2, or 4".into(),
                        });
                    }
                    // 空スライスの as_ptr() は dangling pointer になるため拒否する
                    if vps.is_empty() || sps.is_empty() || pps.is_empty() {
                        return Err(Error::InvalidConfig {
                            field: "parameter_sets".into(),
                            reason: "vps, sps, and pps must not be empty".into(),
                        });
                    }
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
        handler: &H,
    ) -> Result<sys::VTDecompressionSessionRef, Error> {
        unsafe {
            let mut session: sys::VTDecompressionSessionRef = std::ptr::null_mut();
            let record = sys::VTDecompressionOutputCallbackRecord {
                decompressionOutputCallback: Some(Self::output_callback),
                decompressionOutputRefCon: (handler as *const H).cast::<c_void>().cast_mut(),
            };

            let cv_pixel_format = match pixel_format {
                PixelFormat::I420 => sys::kCVPixelFormatType_420YpCbCr8Planar,
                PixelFormat::Nv12 => sys::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            };
            let pf = cf_number_i32(cv_pixel_format as i32)?;
            let dest_attrs = cf_dictionary(&[(sys::kCVPixelBufferPixelFormatTypeKey, pf.0)])?;
            let status = sys::VTDecompressionSessionCreate(
                std::ptr::null_mut(),
                description,
                std::ptr::null_mut(),
                dest_attrs.0.cast(),
                &record,
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
    pub fn decode(&mut self, data: &[u8], user_data: H::UserData) -> Result<(), Error> {
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
            let block_buffer = CfPtrMut(block_buffer_ref);

            let mut sample_buffer_ref = std::ptr::null_mut();
            let status = sys::CMSampleBufferCreateReady(
                std::ptr::null_mut(),
                block_buffer.0,
                self.description,
                1,
                0,
                [].as_ptr(),
                0,
                [].as_ptr(),
                &mut sample_buffer_ref,
            );
            Error::check(status, "CMSampleBufferCreateReady")?;
            let sample_buffer = CfPtrMut(sample_buffer_ref);

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
                let _ = Box::from_raw(source_frame_ref_con.cast::<PendingDecode<H::UserData>>());
                return Err(e);
            }

            Ok(())
        }
    }

    // SAFETY:
    // - `source_frame_ref_con` は `Box<PendingDecode<H::UserData>>` を `Box::into_raw` した
    //   ポインタである。成功時は一度だけ消費し、`VTDecompressionSessionDecodeFrame` 失敗時は
    //   呼び出し側の `Box::from_raw` が回収する。どちらか一方のみが回収する契約である。
    // - デコーダー側は `PendingDecode` を分解して使うため、Box のまま返す。
    unsafe fn take_pending_decode(
        source_frame_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Result<Box<PendingDecode<H::UserData>>, Error> {
        if source_frame_ref_con.is_null() {
            return Err(Error::LimitExceeded {
                reason: format!("{callback_name}: source_frame_ref_con is null"),
            });
        }
        Ok(unsafe { Box::from_raw(source_frame_ref_con.cast::<PendingDecode<H::UserData>>()) })
    }

    unsafe fn callback_from_ref_con<'a>(
        output_callback_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Option<&'a mut H> {
        if output_callback_ref_con.is_null() {
            tracing::error!("{callback_name}: output_callback_ref_con is null");
            return None;
        }
        // SAFETY:
        // - `output_callback_ref_con` は `Box<H>` のヒープアドレスを指す。
        //   `Box<H>` のヒープアドレスは `Decoder` の生存期間中不変である。
        // - FFI コールバックは `&mut H` で排他的にアクセスする。
        Some(unsafe { &mut *output_callback_ref_con.cast::<H>() })
    }

    fn invoke_callback(
        handler: &mut H,
        result: Result<DecodedFrame<H::UserData>, H::Error>,
        callback_name: &'static str,
    ) {
        crate::types::catch_user_panic(callback_name, || handler.on_decoded(result));
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
        let handler =
            unsafe { Self::callback_from_ref_con(decompression_output_ref_con, callback_name) };

        // `source_frame_ref_con` の Box は status の成否にかかわらず必ず回収する。
        // 先に `Error::check(status, ...)` を呼ぶと status エラー時に Box が回収されず
        // リークするため、take を先に実行する。
        let pending =
            match unsafe { Self::take_pending_decode(source_frame_ref_con, callback_name) } {
                Ok(p) => p,
                Err(e) => {
                    if let Some(h) = handler {
                        Self::invoke_callback(h, Err(e.into()), callback_name);
                    }
                    return;
                }
            };

        let Some(handler) = handler else {
            // callback_from_ref_con が null。source_frame_ref_con は take_pending_decode 内で消費済み。
            return;
        };

        let PendingDecode {
            user_data,
            pixel_format,
            ..
        } = *pending;

        if let Err(e) = Error::check(status, callback_name) {
            Self::invoke_callback(handler, Err(e.into()), callback_name);
            return;
        }

        // フレームドロップ等で image_buffer が NULL になる場合がある
        if image_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "decoded image buffer is null".into(),
            };
            Self::invoke_callback(handler, Err(e.into()), callback_name);
            return;
        }

        let buffer = match unsafe { PixelBuffer::new(image_buffer) } {
            Ok(buffer) => buffer,
            Err(e) => {
                Self::invoke_callback(handler, Err(e.into()), callback_name);
                return;
            }
        };
        let frame = match pixel_format {
            PixelFormat::I420 => DecodedFrame::I420 {
                frame: I420Frame { buffer },
                user_data,
            },
            PixelFormat::Nv12 => DecodedFrame::Nv12 {
                frame: Nv12Frame { buffer },
                user_data,
            },
        };
        Self::invoke_callback(handler, Ok(frame), callback_name);
    }
}

impl<H: DecodeHandler> Drop for Decoder<H> {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            tracing::error!("{e}");
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
// handler は Box<H> でヒープに隔離されており、Decoder の生存期間中はアドレス不変である。
unsafe impl<H: DecodeHandler> Send for Decoder<H> {}

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

/// ロック済みのピクセルバッファ
///
/// ピクセルバッファの参照を所有し `CVPixelBufferLockBaseAddress` でロックを保持した状態を表す。
/// 参照の解放とロックの解除は `Drop` が行う。
#[derive(Debug)]
struct PixelBuffer {
    inner: CfPtrMut<sys::__CVBuffer>,
}

impl PixelBuffer {
    /// ピクセルバッファの参照を retain して読み取り専用でロックする
    ///
    /// 返り値はピクセルバッファの参照を所有し、ロックを保持した状態になる。
    ///
    /// # Errors
    ///
    /// `CVPixelBufferLockBaseAddress` が失敗した場合は [`Error::VideoToolbox`] を返す。
    ///
    /// # Safety
    ///
    /// `image_buffer` は有効な `CVImageBufferRef` を指していること。
    unsafe fn new(image_buffer: sys::CVImageBufferRef) -> Result<Self, Error> {
        let retained = unsafe { sys::CFRetain(image_buffer.cast()) };
        let inner = CfPtrMut(retained.cast_mut().cast());

        let flags_readonly = 1;
        let status = unsafe { sys::CVPixelBufferLockBaseAddress(inner.0, flags_readonly) };
        // ロックに失敗した場合も、`inner` の `Drop` が retain した参照を解放する
        Error::check(status, "CVPixelBufferLockBaseAddress")?;

        Ok(Self { inner })
    }

    /// ロック済みプレーンを `&[u8]` として返す
    ///
    /// 乗算オーバーフローまたは内部エラーが発生した場合は空スライスを返す。
    fn plane_slice(&self, plane_index: usize, row_count: usize, bytes_per_row: usize) -> &[u8] {
        // `from_raw_parts` に `usize` で表現できない長さを渡すと未定義動作になるため、
        // 乗算がオーバーフローした場合は空スライスを返す
        let Some(len) = row_count.checked_mul(bytes_per_row) else {
            return &[];
        };
        let ptr = unsafe {
            sys::CVPixelBufferGetBaseAddressOfPlane(self.inner.0, plane_index) as *const u8
        };
        // 非プラナーのピクセルバッファの場合、この関数は NULL を返す (Apple のドキュメント)。
        // NULL を `from_raw_parts` に渡すと未定義動作になるため、空スライスを返す
        if ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    /// 指定プレーンのストライド（1 行あたりのバイト数）を返す
    fn plane_stride(&self, plane_index: usize) -> usize {
        unsafe { sys::CVPixelBufferGetBytesPerRowOfPlane(self.inner.0, plane_index) }
    }

    /// フレームの Y 成分のデータを返す
    fn y_plane(&self) -> &[u8] {
        self.plane_slice(0, self.height(), self.plane_stride(0))
    }

    /// フレームの幅を返す
    fn width(&self) -> usize {
        unsafe { sys::CVPixelBufferGetWidth(self.inner.0) }
    }

    /// フレームの高さを返す
    fn height(&self) -> usize {
        unsafe { sys::CVPixelBufferGetHeight(self.inner.0) }
    }
}

impl Drop for PixelBuffer {
    fn drop(&mut self) {
        unsafe {
            let flags_readonly = 1;
            sys::CVPixelBufferUnlockBaseAddress(self.inner.0, flags_readonly);
        }
    }
}

/// I420 形式のデコード済みフレーム (3 プレーン: Y, U, V)
///
/// ## プレーン参照について
///
/// 各 `*_plane` は乗算オーバーフローしたり内部関数でのエラーが発生した時は
/// 空のスライスを返す防御を入れている。
/// そのため空スライスは「デコードが成功したがピクセルが無い」を意味しない。
#[derive(Debug)]
pub struct I420Frame {
    /// ロック済みピクセルバッファ
    buffer: PixelBuffer,
}

impl I420Frame {
    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        self.buffer.y_plane()
    }

    /// フレームの U 成分のデータを返す
    pub fn u_plane(&self) -> &[u8] {
        self.buffer
            .plane_slice(1, self.height().div_ceil(2), self.u_stride())
    }

    /// フレームの V 成分のデータを返す
    pub fn v_plane(&self) -> &[u8] {
        self.buffer
            .plane_slice(2, self.height().div_ceil(2), self.v_stride())
    }

    /// フレームの Y 成分のストライドを返す
    pub fn y_stride(&self) -> usize {
        self.buffer.plane_stride(0)
    }

    /// フレームの U 成分のストライドを返す
    pub fn u_stride(&self) -> usize {
        self.buffer.plane_stride(1)
    }

    /// フレームの V 成分のストライドを返す
    pub fn v_stride(&self) -> usize {
        self.buffer.plane_stride(2)
    }

    /// フレームの幅を返す
    pub fn width(&self) -> usize {
        self.buffer.width()
    }

    /// フレームの高さを返す
    pub fn height(&self) -> usize {
        self.buffer.height()
    }
}

/// NV12 形式のデコード済みフレーム (2 プレーン: Y, UV interleaved)
///
/// ## プレーン参照について
///
/// プレーンの参照に関する契約は [`I420Frame`] と同じ。
#[derive(Debug)]
pub struct Nv12Frame {
    /// ロック済みピクセルバッファ
    buffer: PixelBuffer,
}

impl Nv12Frame {
    /// フレームの Y 成分のデータを返す
    pub fn y_plane(&self) -> &[u8] {
        self.buffer.y_plane()
    }

    /// フレームの UV インターリーブデータを返す
    pub fn uv_plane(&self) -> &[u8] {
        self.buffer
            .plane_slice(1, self.height().div_ceil(2), self.uv_stride())
    }

    /// フレームの Y 成分のストライドを返す
    pub fn y_stride(&self) -> usize {
        self.buffer.plane_stride(0)
    }

    /// フレームの UV インターリーブのストライドを返す
    pub fn uv_stride(&self) -> usize {
        self.buffer.plane_stride(1)
    }

    /// フレームの幅を返す
    pub fn width(&self) -> usize {
        self.buffer.width()
    }

    /// フレームの高さを返す
    pub fn height(&self) -> usize {
        self.buffer.height()
    }
}
