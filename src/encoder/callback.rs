//! エンコード出力コールバックとパラメータセット抽出

use std::ffi::c_void;

use crate::{
    encoder::{Encoder, frame::EncodedFrame, handler::EncodeHandler},
    error::Error,
    sys,
};

// パラメータセット (VPS, SPS, PPS) のタプル型
type ParameterSets = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>);

impl<H: EncodeHandler> Encoder<H> {
    // SAFETY:
    // - `source_frame_ref_con` は `Box<H::UserData>` を `Box::into_raw` したポインタである。
    //   成功時は一度だけ消費し、`VTCompressionSessionEncodeFrame` 失敗時は呼び出し側の
    //   `Box::from_raw` が回収する。どちらか一方のみが回収する契約である。
    // - エンコーダー側は `EncodedFrame` に値のまま載せるため、`Box` からムーブアウトする。
    unsafe fn take_user_data(
        source_frame_ref_con: *mut c_void,
        callback_name: &'static str,
    ) -> Result<H::UserData, Error> {
        if source_frame_ref_con.is_null() {
            return Err(Error::LimitExceeded {
                reason: format!("{callback_name}: source_frame_ref_con is null"),
            });
        }
        Ok(unsafe { *Box::from_raw(source_frame_ref_con.cast::<H::UserData>()) })
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
        //   `Box<H>` のヒープアドレスは `Encoder` の生存期間中不変である。
        // - FFI コールバックは `&mut H` で排他的にアクセスする。
        Some(unsafe { &mut *output_callback_ref_con.cast::<H>() })
    }

    fn invoke_callback(handler: &mut H, result: Result<EncodedFrame<H::UserData>, H::Error>) {
        handler.on_encoded(result);
    }

    /// VTCompressionSessionCreate に渡す H.264 用の出力コールバック
    ///
    /// `outputCallbackRefCon` は `Box<H>` のヒープアドレスを指し、`&mut H` として復元する。
    /// `sourceFrameRefCon` は `Box<H::UserData>` を指し、`process_encoded_output` 内で回収する。
    pub(super) unsafe extern "C" fn output_callback_h264(
        output_callback_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
                source_frame_ref_con,
                sample_buffer,
                status,
                "output_callback_h264",
                Self::extract_h264_params,
            );
        }
    }

    /// VTCompressionSessionCreate に渡す H.265 用の出力コールバック
    ///
    /// `outputCallbackRefCon` は `Box<H>` のヒープアドレスを指し、`&mut H` として復元する。
    /// `sourceFrameRefCon` は `Box<H::UserData>` を指し、`process_encoded_output` 内で回収する。
    pub(super) unsafe extern "C" fn output_callback_h265(
        output_callback_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: i32,
        _info_flags: sys::VTEncodeInfoFlags,
        sample_buffer: sys::CMSampleBufferRef,
    ) {
        unsafe {
            Self::process_encoded_output(
                output_callback_ref_con,
                source_frame_ref_con,
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
        source_frame_ref_con: *mut c_void,
        sample_buffer: sys::CMSampleBufferRef,
        status: i32,
        callback_name: &'static str,
        extract_params: unsafe fn(sys::CMVideoFormatDescriptionRef) -> Result<ParameterSets, Error>,
    ) {
        let handler =
            unsafe { Self::callback_from_ref_con(output_callback_ref_con, callback_name) };

        // `source_frame_ref_con` の Box は status の成否にかかわらず必ず回収する。
        // 先に `Error::check(status, ...)` を呼ぶと status エラー時に Box が回収されず
        // リークするため、take を先に実行する。
        let user_data = match unsafe { Self::take_user_data(source_frame_ref_con, callback_name) } {
            Ok(data) => data,
            Err(e) => {
                if let Some(h) = handler {
                    Self::invoke_callback(h, Err(e.into()));
                }
                return;
            }
        };

        let Some(handler) = handler else {
            // callback_from_ref_con が null。source_frame_ref_con は take_user_data 内で消費済み。
            return;
        };

        if let Err(e) = Error::check(status, callback_name) {
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        // フレームドロップ等で sample_buffer が NULL になる場合がある
        if sample_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "encoded sample buffer is null".into(),
            };
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        unsafe {
            let data_buffer = sys::CMSampleBufferGetDataBuffer(sample_buffer);
            if data_buffer.is_null() {
                let e = Error::LimitExceeded {
                    reason: "CMSampleBufferGetDataBuffer returned null".into(),
                };
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }
            // `CMBlockBufferGetDataPointer` の戻り長はオフセットからの連続領域長であり、ブロック全体長ではない。
            // 非連続バッファでは `data_pointer_len < block_len` になり得るため、`CMBlockBufferCopyDataBytes` で全長をコピーする。
            let block_len = sys::CMBlockBufferGetDataLength(data_buffer);
            if block_len > MAX_ENCODED_BLOCK_COPY_BYTES {
                let e = Error::LimitExceeded {
                    reason: format!(
                        "CMBlockBufferGetDataLength {block_len} exceeds defensive maximum {MAX_ENCODED_BLOCK_COPY_BYTES}"
                    ),
                };
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }
            let mut data = vec![0u8; block_len];
            let status = sys::CMBlockBufferCopyDataBytes(
                data_buffer,
                0,
                block_len,
                data.as_mut_ptr().cast(),
            );
            if let Err(e) = Error::check(status, "CMBlockBufferCopyDataBytes") {
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }

            let description = sys::CMSampleBufferGetFormatDescription(sample_buffer);
            let keyframe = is_keyframe(sample_buffer);

            let (vps_list, sps_list, pps_list) = if keyframe {
                if description.is_null() {
                    let e = Error::LimitExceeded {
                        reason: "CMSampleBufferGetFormatDescription returned null for keyframe"
                            .into(),
                    };
                    Self::invoke_callback(handler, Err(e.into()));
                    return;
                }
                match extract_params(description) {
                    Ok(params) => params,
                    Err(e) => {
                        Self::invoke_callback(handler, Err(e.into()));
                        return;
                    }
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
                user_data,
            };
            Self::invoke_callback(handler, Ok(frame));
        }
    }

    /// H.264 のパラメータセット (SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル (H.264 では vps_list は空)
    unsafe fn extract_h264_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Result<ParameterSets, Error> {
        unsafe {
            if description.is_null() {
                return Err(Error::LimitExceeded {
                    reason: "CMVideoFormatDescription is null in extract_h264_params".into(),
                });
            }
            let mut nalu_header_length = 0;
            let status = sys::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                description,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut nalu_header_length,
            );
            Error::check(status, "CMVideoFormatDescriptionGetH264ParameterSetAtIndex")?;
            if nalu_header_length != 4 {
                return Err(Error::LimitExceeded {
                    reason: format!("unexpected NAL unit header length: {nalu_header_length}"),
                });
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
                Error::check(status, "CMVideoFormatDescriptionGetH264ParameterSetAtIndex")?;
            }

            let sps_vec = vec_u8_from_raw_parts_safe(sps_ptr, sps_size, "H264 SPS")?;
            let pps_vec = vec_u8_from_raw_parts_safe(pps_ptr, pps_size, "H264 PPS")?;

            Ok((Vec::new(), vec![sps_vec], vec![pps_vec]))
        }
    }

    /// H.265 のパラメータセット (VPS, SPS, PPS) を抽出する
    ///
    /// 戻り値は (vps_list, sps_list, pps_list) のタプル
    unsafe fn extract_h265_params(
        description: sys::CMVideoFormatDescriptionRef,
    ) -> Result<ParameterSets, Error> {
        unsafe {
            if description.is_null() {
                return Err(Error::LimitExceeded {
                    reason: "CMVideoFormatDescription is null in extract_h265_params".into(),
                });
            }
            let mut nalu_header_length = 0;
            let status = sys::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                description,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut nalu_header_length,
            );
            Error::check(status, "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex")?;
            if nalu_header_length != 4 {
                return Err(Error::LimitExceeded {
                    reason: format!("unexpected NAL unit header length: {nalu_header_length}"),
                });
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
                Error::check(status, "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex")?;
            }

            let vps_vec = vec_u8_from_raw_parts_safe(vps_ptr, vps_size, "HEVC VPS")?;
            let sps_vec = vec_u8_from_raw_parts_safe(sps_ptr, sps_size, "HEVC SPS")?;
            let pps_vec = vec_u8_from_raw_parts_safe(pps_ptr, pps_size, "HEVC PPS")?;

            Ok((vec![vps_vec], vec![sps_vec], vec![pps_vec]))
        }
    }
}

/// パラメータセット 1 個あたりのコピー上限（バイト）。
///
/// **根拠（レビューで追試可能）**: ISO/IEC 14496-15（MPEG-4 Part 15）において、
/// `AVCDecoderConfigurationRecord` の `sequenceParameterSetLength` / `pictureParameterSetLength`、
/// および `HEVCDecoderConfigurationRecord` 内の各 NAL の長さは **`unsigned int(16)`** で表される。
/// よって 1 パラメータセットあたり表現可能な最大長は **65535 バイト**（`2^16 - 1`）である。
/// 本クレートが扱う AVCC 互換のパラメータセット長と整合する防御的上限とする（拒否のみ。クランプはビットストリームを壊す）。
///
/// Annex B バイトストリーム上の NAL がこれを超える理論ケースは、本上限では拒否される（実運用では稀）。
const MAX_PARAMETER_SET_COPY_BYTES: usize = u16::MAX as usize;

/// エンコード出力 1 フレーム分を `Vec` にコピーするときの防御的上限（バイト）。
///
/// `MAX_PARAMETER_SET_COPY_BYTES`（パラメータセット用）とは別。`CMBlockBufferGetDataLength` が異常に大きい場合の OOM を防ぐ。
/// 値は保守的に大きめ（4K・高ビットレート等を想定）。超過時はエラーを通知して当該フレームを破棄する。
const MAX_ENCODED_BLOCK_COPY_BYTES: usize = 256 * 1024 * 1024;

/// `slice::from_raw_parts` の前提（長さ 0 でも非 NULL ポインタ、長さ正では NULL 禁止）を満たすためのヘルパー
fn vec_u8_from_raw_parts_safe(
    ptr: *const u8,
    len: usize,
    context: &'static str,
) -> Result<Vec<u8>, Error> {
    if len > MAX_PARAMETER_SET_COPY_BYTES {
        return Err(Error::LimitExceeded {
            reason: format!(
                "{context}: parameter set length {len} exceeds defensive maximum {MAX_PARAMETER_SET_COPY_BYTES}"
            ),
        });
    }
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(Error::LimitExceeded {
            reason: format!("{context}: null pointer with non-zero length"),
        });
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len).to_vec() })
}

fn is_keyframe(sample_buffer: sys::CMSampleBufferRef) -> bool {
    unsafe {
        let attachments = sys::CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 1);
        if attachments.is_null() {
            return false;
        }

        if sys::CFArrayGetCount(attachments) == 0 {
            return false;
        }

        let attachment = sys::CFArrayGetValueAtIndex(attachments, 0);
        if attachment.is_null() {
            return false;
        }
        // 添付が CFDictionary でない場合は NotSync キーを解釈できない
        if sys::CFGetTypeID(attachment as sys::CFTypeRef) != sys::CFDictionaryGetTypeID() {
            return false;
        }

        let not_sync = sys::CFDictionaryGetValue(
            attachment as *mut _,
            sys::kCMSampleAttachmentKey_NotSync as *const c_void,
        );
        not_sync != sys::kCFBooleanTrue as *const c_void
    }
}
