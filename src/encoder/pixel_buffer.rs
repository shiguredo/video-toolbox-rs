//! CVPixelBuffer の操作とフレーム送信

use std::ffi::c_void;

use crate::{
    encoder::{Encoder, config::EncodeOptions, frame::FrameData, handler::EncodeHandler},
    error::Error,
    sys,
    types::{CfPtrMut, CvPixelBufferUnlockGuard, PixelFormat, cf_dictionary},
};

impl<H: EncodeHandler> Encoder<H> {
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
                    reason: "CVPixelBuffer base address for plane is null".into(),
                });
            }
            let dst_stride = sys::CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, plane_index);
            let cv_plane_height = sys::CVPixelBufferGetHeightOfPlane(pixel_buffer, plane_index);
            let cv_plane_width = sys::CVPixelBufferGetWidthOfPlane(pixel_buffer, plane_index);
            // 行あたり `dst_stride` バイトしかないのに `src_width` バイトを書くとバッファ外になる
            if dst_stride < src_width {
                return Err(Error::LimitExceeded {
                    reason: "plane destination stride is less than copy width".into(),
                });
            }
            // Core Video の契約では、プレーンは少なくとも `height * bytesPerRow` バイトを指す。
            // コピー範囲が `CVPixelBuffer` が報告するプレーン寸法を超えないことを検証する。
            if src_width > cv_plane_width || src_height > cv_plane_height {
                return Err(Error::LimitExceeded {
                    reason: "plane copy dimensions exceed CVPixelBuffer plane bounds".into(),
                });
            }
            let plane_storage =
                dst_stride
                    .checked_mul(cv_plane_height)
                    .ok_or(Error::LimitExceeded {
                        reason: "plane storage byte length overflow".into(),
                    })?;
            let write_span = if src_height == 0 {
                0
            } else if dst_stride == src_width {
                src_width
                    .checked_mul(src_height)
                    .ok_or(Error::LimitExceeded {
                        reason: "plane copy byte length overflow".into(),
                    })?
            } else {
                src_height
                    .checked_sub(1)
                    .and_then(|r| r.checked_mul(dst_stride))
                    .and_then(|o| o.checked_add(src_width))
                    .ok_or(Error::LimitExceeded {
                        reason: "plane row copy span overflow".into(),
                    })?
            };
            if write_span > plane_storage {
                return Err(Error::LimitExceeded {
                    reason: "plane copy would exceed CVPixelBuffer plane storage".into(),
                });
            }

            let copy_size = src_width
                .checked_mul(src_height)
                .ok_or(Error::LimitExceeded {
                    reason: "plane copy byte length overflow".into(),
                })?;
            if dst_stride == src_width {
                // ストライドと入力幅が一致する場合は一括コピー
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst, copy_size);
            } else {
                // ストライドが異なる場合は行ごとにコピー
                for row in 0..src_height {
                    let src_off = row.checked_mul(src_width).ok_or(Error::LimitExceeded {
                        reason: "plane row source offset overflow".into(),
                    })?;
                    let dst_off = row.checked_mul(dst_stride).ok_or(Error::LimitExceeded {
                        reason: "plane row destination offset overflow".into(),
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
                        plane: "Y".into(),
                        expected: y_expected,
                        actual: y.len(),
                    });
                }
                if u.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "U".into(),
                        expected: uv_expected,
                        actual: u.len(),
                    });
                }
                if v.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "V".into(),
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
                        plane: "Y".into(),
                        expected: y_expected,
                        actual: y.len(),
                    });
                }
                if uv.len() < uv_expected {
                    return Err(Error::InsufficientFrameData {
                        plane: "UV".into(),
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
            reason: "frame dimension size overflow".into(),
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

        // PTS のオーバーフロー検査はリソース確保・送信より前に行う。オーバーフロー時はフレームを
        // 送信せずに `next_input_pts` を変更しないままエラーを返す (送信後に検査すると、フレームが
        // in-flight のまま失敗扱いになり、次回呼び出しで同一 PTS が再送される)。
        // 入力検証 (pixel_format / データ長) を先に行い、その後に検査することで
        // 無駄なバッファ確保・コピーを避けつつエラー優先順位を維持する。
        let new_next_input_pts = self
            .next_input_pts
            .checked_add(self.config.fps_denominator as i64)
            .ok_or(Error::LimitExceeded {
                reason: "input presentation timestamp overflow".into(),
            })?;

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
                Some(cf_dictionary(&[(
                    sys::kVTEncodeFrameOptionKey_ForceKeyFrame,
                    sys::kCFBooleanTrue as *const c_void,
                )])?)
            } else {
                None
            };
            let frame_properties_ptr = frame_properties
                .as_ref()
                .map_or(std::ptr::null(), |g| g.0.cast());
            let source_frame_ref_con = Box::into_raw(Box::new(user_data)).cast::<c_void>();

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties_ptr,
                source_frame_ref_con,
                std::ptr::null_mut(),
            );
            if let Err(e) = Error::check(status, "VTCompressionSessionEncodeFrame") {
                // status エラー時は sourceFrameRefCon がコールバックされないため、ここで drop する
                let _ = Box::from_raw(source_frame_ref_con.cast::<H::UserData>());
                return Err(e);
            }

            self.next_input_pts = new_next_input_pts;

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
    /// また [`crate::encoder::config::EncoderConfig`] の解像度・プレーン構成と整合するピクセルバッファであることは **呼び出し側の責務**とする。
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
                    // I420 / Nv12 のいずれでもない FourCC は不一致ではなく未知として区別する
                    return Err(Error::UnknownPixelFormat {
                        expected: self.config.pixel_format,
                        fourcc: format_type,
                    });
                }
            };
            if actual != self.config.pixel_format {
                return Err(Error::PixelFormatMismatch {
                    expected: self.config.pixel_format,
                    actual,
                });
            }

            // PTS のオーバーフロー検査はリソース確保・送信より前に行う。オーバーフロー時はフレームを
            // 送信せずに `next_input_pts` を変更しないままエラーを返す (送信後に検査すると、フレームが
            // in-flight のまま失敗扱いになり、次回呼び出しで同一 PTS が再送される)。
            // 入力検証 (ピクセルフォーマット) を先に行い、その後に検査することで
            // 無駄な CFRetain を避けつつエラー優先順位を維持する。
            let new_next_input_pts = self
                .next_input_pts
                .checked_add(self.config.fps_denominator as i64)
                .ok_or(Error::LimitExceeded {
                    reason: "input presentation timestamp overflow".into(),
                })?;

            // CFRetain して CfPtrMut でラップ（スコープ終了時に CFRelease される）
            sys::CFRetain(pixel_buffer_ptr.cast());
            let image_buffer = CfPtrMut(pixel_buffer_ptr.cast::<sys::__CVBuffer>());

            let frame_properties = if options.force_key_frame {
                Some(cf_dictionary(&[(
                    sys::kVTEncodeFrameOptionKey_ForceKeyFrame,
                    sys::kCFBooleanTrue as *const c_void,
                )])?)
            } else {
                None
            };
            let frame_properties_ptr = frame_properties
                .as_ref()
                .map_or(std::ptr::null(), |g| g.0.cast());
            let source_frame_ref_con = Box::into_raw(Box::new(user_data)).cast::<c_void>();

            let status = sys::VTCompressionSessionEncodeFrame(
                self.session,
                image_buffer.0,
                sys::CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32),
                sys::kCMTimeInvalid,
                frame_properties_ptr,
                source_frame_ref_con,
                std::ptr::null_mut(),
            );
            if let Err(e) = Error::check(status, "VTCompressionSessionEncodeFrame") {
                let _ = Box::from_raw(source_frame_ref_con.cast::<H::UserData>());
                return Err(e);
            }

            self.next_input_pts = new_next_input_pts;

            Ok(())
        }
    }
}
