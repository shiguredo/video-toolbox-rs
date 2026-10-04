//! エンコード出力コールバックとパラメータセット抽出

use std::ffi::c_void;

use crate::{
    encoder::{
        EncodeCallbackContext, Encoder,
        frame::{EncodedFrame, PictureType, Timestamp},
        handler::EncodeHandler,
    },
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
    ) -> Option<&'a mut EncodeCallbackContext<H>> {
        if output_callback_ref_con.is_null() {
            tracing::error!("{callback_name}: output_callback_ref_con is null");
            return None;
        }
        // SAFETY:
        // - `output_callback_ref_con` は `Box<EncodeCallbackContext<H>>` のヒープアドレスを指す。
        //   `Box` のヒープアドレスは `Encoder` の生存期間中不変である。
        // - FFI コールバックは `&mut EncodeCallbackContext<H>` で排他的にアクセスする。
        Some(unsafe { &mut *output_callback_ref_con.cast::<EncodeCallbackContext<H>>() })
    }

    /// ハンドラーへ結果を通知し、対応する統計値を計上する
    ///
    /// ユーザーハンドラーの panic は [`crate::types::catch_user_panic`] が捕捉するため、
    /// この関数は必ず戻る。`Ok` は出力フレーム数、`Err` はエラー数として計上する。
    /// 計上はハンドラーの実行前に行う。ハンドラーが結果を外部へ公開した直後に
    /// 利用側が統計値を読んでも、その結果ぶんが計上済みであるようにするため。
    fn invoke_callback(
        context: &mut EncodeCallbackContext<H>,
        result: Result<EncodedFrame<H::UserData>, H::Error>,
        callback_name: &'static str,
    ) {
        if result.is_ok() {
            context.stats.total_output_frame_count.inc();
        } else {
            context.stats.total_error_count.inc();
        }
        crate::types::catch_user_panic(callback_name, || context.handler.on_encoded(result));
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
        let context =
            unsafe { Self::callback_from_ref_con(output_callback_ref_con, callback_name) };

        // `source_frame_ref_con` の Box は status の成否にかかわらず必ず回収する。
        // 先に `Error::check(status, ...)` を呼ぶと status エラー時に Box が回収されず
        // リークするため、take を先に実行する。
        let user_data = match unsafe { Self::take_user_data(source_frame_ref_con, callback_name) } {
            Ok(data) => data,
            Err(e) => {
                if let Some(context) = context {
                    Self::invoke_callback(context, Err(e.into()), callback_name);
                }
                return;
            }
        };

        let Some(context) = context else {
            // callback_from_ref_con が null。source_frame_ref_con は take_user_data 内で消費済み。
            // この経路は Video Toolbox が refcon を渡さなかった場合に限られ、
            // 統計値を参照できないため in_flight_frames の減算も行わない。
            return;
        };

        // ユーザーデータを回収した時点でフレームは Video Toolbox の管理から外れるため、
        // ユーザーハンドラーの実行前に in-flight フレーム数を減らす。
        context.stats.in_flight_frames.dec();

        if let Err(e) = Error::check(status, callback_name) {
            Self::invoke_callback(context, Err(e.into()), callback_name);
            return;
        }

        // フレームドロップ等で sample_buffer が NULL になる場合がある
        if sample_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "encoded sample buffer is null".into(),
            };
            Self::invoke_callback(context, Err(e.into()), callback_name);
            return;
        }

        unsafe {
            let data_buffer = sys::CMSampleBufferGetDataBuffer(sample_buffer);
            if data_buffer.is_null() {
                let e = Error::LimitExceeded {
                    reason: "CMSampleBufferGetDataBuffer returned null".into(),
                };
                Self::invoke_callback(context, Err(e.into()), callback_name);
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
                Self::invoke_callback(context, Err(e.into()), callback_name);
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
                Self::invoke_callback(context, Err(e.into()), callback_name);
                return;
            }

            let description = sys::CMSampleBufferGetFormatDescription(sample_buffer);
            let timestamp = presentation_timestamp(sample_buffer);
            let picture_type = SampleAttachments::from_sample_buffer(sample_buffer).picture_type();
            let keyframe = picture_type == PictureType::I;

            // パラメータセットはキーフレームにだけ付くため、内部でもピクチャータイプで判定する
            let (vps_list, sps_list, pps_list) = if keyframe {
                if description.is_null() {
                    let e = Error::LimitExceeded {
                        reason: "CMSampleBufferGetFormatDescription returned null for keyframe"
                            .into(),
                    };
                    Self::invoke_callback(context, Err(e.into()), callback_name);
                    return;
                }
                match extract_params(description) {
                    Ok(params) => params,
                    Err(e) => {
                        Self::invoke_callback(context, Err(e.into()), callback_name);
                        return;
                    }
                }
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };

            let frame = EncodedFrame {
                timestamp,
                picture_type,
                sps_list,
                pps_list,
                vps_list,
                data,
                user_data,
            };
            Self::invoke_callback(context, Ok(frame), callback_name);
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

/// 出力サンプルの先頭に添付されたフレーム種別の辞書
///
/// 保持しているのはサンプルバッファが所有する添付辞書への参照であり、サンプルバッファを
/// 解放すると無効になる。参照するのは [`Encoder::process_encoded_output`] が
/// サンプルバッファを扱っている間だけに限る。添付が無い場合、または添付が辞書でない場合は、
/// キーを解釈できないため各キーを読むメソッドはすべて既定値を返す。
#[derive(Clone, Copy)]
struct SampleAttachments {
    dictionary: sys::CFDictionaryRef,
}

impl SampleAttachments {
    /// 出力サンプルの先頭の添付辞書から [`SampleAttachments`] を作る
    ///
    /// 添付が無い場合・添付が CFDictionary でない場合は、キーを解釈できないため
    /// NULL を保持したまま返す。
    unsafe fn from_sample_buffer(sample_buffer: sys::CMSampleBufferRef) -> Self {
        // SAFETY: 添付配列と添付辞書はサンプルバッファが所有しており、参照するのは
        // `process_encoded_output` がサンプルバッファを扱っている間だけである。
        unsafe {
            let attachments = sys::CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 1);
            if attachments.is_null() || sys::CFArrayGetCount(attachments) == 0 {
                return Self {
                    dictionary: std::ptr::null(),
                };
            }

            let attachment = sys::CFArrayGetValueAtIndex(attachments, 0);
            if attachment.is_null()
                || sys::CFGetTypeID(attachment as sys::CFTypeRef) != sys::CFDictionaryGetTypeID()
            {
                return Self {
                    dictionary: std::ptr::null(),
                };
            }

            Self {
                dictionary: attachment as sys::CFDictionaryRef,
            }
        }
    }

    /// `key` に対応する CFBoolean の値を返す
    ///
    /// キーが無い場合、値が CFBoolean でない場合、または添付辞書を取得できていない場合は
    /// `default` を返す。
    unsafe fn bool_value(&self, key: sys::CFStringRef, default: bool) -> bool {
        if self.dictionary.is_null() {
            return default;
        }
        unsafe {
            let value = sys::CFDictionaryGetValue(self.dictionary, key as *const c_void);
            if value.is_null()
                || sys::CFGetTypeID(value as sys::CFTypeRef) != sys::CFBooleanGetTypeID()
            {
                return default;
            }
            sys::CFBooleanGetValue(value as sys::CFBooleanRef) != 0
        }
    }

    /// 他のフレームを参照しない同期サンプル (キーフレーム) かどうかを返す
    ///
    /// 添付の `NotSync` キーが `true` の場合に非同期サンプルと判定し、キーが無い場合は
    /// 同期サンプルと判定する。
    fn is_sync_sample(&self) -> bool {
        !unsafe { self.bool_value(sys::kCMSampleAttachmentKey_NotSync, false) }
    }

    /// ピクチャータイプを判定する
    ///
    /// 添付から同期サンプルかどうかを判定し、同期サンプルを [`PictureType::I`] とする。
    /// 同期サンプル以外の判定の根拠は [`PictureType`] の各バリアントの説明を参照すること。
    fn picture_type(&self) -> PictureType {
        // 同期サンプルは他のフレームを参照しないため、IDR フレームと区別できない
        if self.is_sync_sample() {
            return PictureType::I;
        }
        if self.dictionary.is_null() {
            // キーフレームでも辞書でもないサンプルはフレーム種別を判定できない
            return PictureType::Unknown;
        }
        if !unsafe { self.bool_value(sys::kCMSampleAttachmentKey_DependsOnOthers, true) } {
            // 非同期サンプルなのに他のフレームを参照しないサンプルは、I フレームの
            // 定義 (他のフレームを参照しない) に反するため解釈しない
            return PictureType::Unknown;
        }
        // 他のフレームから参照されないサンプルは提示順序を入れ替えても他のフレームに
        // 影響しないため、B フレームと判定する
        if !unsafe { self.bool_value(sys::kCMSampleAttachmentKey_IsDependedOnByOthers, true) } {
            return PictureType::B;
        }
        PictureType::P
    }
}

/// 出力サンプルの提示時刻を返す
///
/// 提示時刻を時刻として解釈できない場合、つまり `CMTime` の `flags` に
/// `kCMTimeFlags_Valid` が立っていない場合、または不定・無限を表すフラグが立っている場合は
/// `None` を返す。0 を返すと先頭フレームと区別できなくなるため、`Some` にはしない。
fn presentation_timestamp(sample_buffer: sys::CMSampleBufferRef) -> Option<Timestamp> {
    unsafe {
        let time = sys::CMSampleBufferGetPresentationTimeStamp(sample_buffer);
        // CMTime は packed のため、フィールドは一度ローカルへコピーしてから参照する
        let (value, timescale, flags) = (time.value, time.timescale, time.flags);
        if flags & sys::kCMTimeFlags_Valid == 0
            || flags & sys::kCMTimeFlags_ImpliedValueFlagsMask != 0
        {
            return None;
        }
        Some(Timestamp { value, timescale })
    }
}

#[cfg(test)]
mod tests {
    //! 出力サンプルから時刻を取り出す処理のうち、Video Toolbox の実出力では再現できない
    //! ケース (CMTime が有効でない場合) を直接確認するためのテスト。

    use super::*;
    use crate::types::CfPtrMut;

    /// 提示時刻が有効でない出力サンプルを組み立てる
    ///
    /// `CMSampleBufferCreateReady` は `kCMTimeInvalid` (flags = 0) の提示時刻も受け付けるため、
    /// Video Toolbox がフレームのドロップなどで無効な時刻を通知する場合を再現できる。
    fn sample_buffer_with_invalid_pts() -> CfPtrMut<sys::opaqueCMSampleBuffer> {
        // データを持たない CMBlockBuffer は生成できないため、長さ 1 の静的領域を指す
        // CMBlockBuffer を生成する (CoreMedia はデコード時のみ参照する)
        static DATA: [u8; 1] = [0];
        unsafe {
            let mut block_buffer_ref = std::ptr::null_mut();
            let status = sys::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null_mut(),
                DATA.as_ptr().cast_mut().cast(),
                DATA.len(),
                sys::kCFAllocatorNull,
                std::ptr::null(),
                0,
                DATA.len(),
                0,
                &mut block_buffer_ref,
            );
            Error::check(status, "CMBlockBufferCreateWithMemoryBlock")
                .expect("データを持つ CMBlockBuffer を生成できること");
            let block_buffer = CfPtrMut(block_buffer_ref);

            let mut sample_buffer_ref = std::ptr::null_mut();
            let mut timing = sys::CMSampleTimingInfo {
                duration: sys::kCMTimeInvalid,
                presentationTimeStamp: sys::kCMTimeInvalid,
                decodeTimeStamp: sys::kCMTimeInvalid,
            };
            let status = sys::CMSampleBufferCreateReady(
                std::ptr::null_mut(),
                block_buffer.0,
                std::ptr::null(),
                1,
                1,
                (&mut timing) as *mut sys::CMSampleTimingInfo,
                0,
                std::ptr::null(),
                &mut sample_buffer_ref,
            );
            Error::check(status, "CMSampleBufferCreateReady")
                .expect("無効な提示時刻のサンプルバッファを生成できること");
            CfPtrMut(sample_buffer_ref)
        }
    }

    /// 提示時刻が有効でない場合に `None` が返ることを検証する
    ///
    /// 0 を返すと先頭フレーム (提示時刻 0) と区別できなくなるため、`Option` で
    /// 無効であることを表す契約になっている。
    #[test]
    fn presentation_timestamp_is_none_when_time_is_invalid() {
        let sample_buffer = sample_buffer_with_invalid_pts();
        let timestamp = presentation_timestamp(sample_buffer.0);
        assert!(
            timestamp.is_none(),
            "有効でない提示時刻からは時刻を返さないこと: {timestamp:?}"
        );
    }

    /// フレーム種別のキーを持たないサンプルを同期サンプルとして扱うことを検証する
    ///
    /// Video Toolbox は出力サンプルにフレーム種別の添付辞書を付けるため、この経路は
    /// 実出力では通らない。キーが無い添付辞書を「非同期サンプル」と解釈すると、
    /// キーフレームでないことだけを根拠に P フレームと誤判定してしまうため、
    /// キーが無い場合は同期サンプル (キーフレーム) として扱う契約を確認する。
    #[test]
    fn attachments_without_keys_are_treated_as_sync_sample() {
        let sample_buffer = sample_buffer_with_invalid_pts();
        let attachments = unsafe { SampleAttachments::from_sample_buffer(sample_buffer.0) };
        assert!(
            attachments.is_sync_sample(),
            "NotSync キーが無いサンプルは同期サンプルとして扱うこと"
        );
        assert_eq!(
            attachments.picture_type(),
            PictureType::I,
            "同期サンプルのピクチャータイプは I になること"
        );
    }
}
