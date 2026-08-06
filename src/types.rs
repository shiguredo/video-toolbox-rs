use std::ffi::c_void;

use crate::{error::Error, sys};

/// Video Toolbox / CoreMedia の `i32` 寸法引数に渡す前に、`u32` が正の `i32` に収まることを検証する。
pub(crate) fn validate_video_dimensions_for_toolbox(width: u32, height: u32) -> Result<(), Error> {
    let max = i32::MAX as u32;
    if width == 0 {
        return Err(Error::InvalidConfig {
            field: "width".into(),
            reason: "must not be zero".into(),
        });
    }
    if height == 0 {
        return Err(Error::InvalidConfig {
            field: "height".into(),
            reason: "must not be zero".into(),
        });
    }
    if width > max {
        return Err(Error::InvalidConfig {
            field: "width".into(),
            reason: "must fit in i32 for Video Toolbox dimensions".into(),
        });
    }
    if height > max {
        return Err(Error::InvalidConfig {
            field: "height".into(),
            reason: "must fit in i32 for Video Toolbox dimensions".into(),
        });
    }
    Ok(())
}

/// ピクセルフォーマット
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// kCVPixelFormatType_420YpCbCr8Planar (3 プレーン: Y, U, V)
    I420,
    /// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange (2 プレーン: Y, UV interleaved)
    Nv12,
}

/// `CVPixelBufferLockBaseAddress` の後に束ね、`Drop` で必ず `CVPixelBufferUnlockBaseAddress` を呼ぶ。
/// `copy_plane` が `Err` でもロック解除を漏らさない。
pub(crate) struct CvPixelBufferUnlockGuard(pub(crate) sys::CVPixelBufferRef);

impl Drop for CvPixelBufferUnlockGuard {
    fn drop(&mut self) {
        unsafe {
            let status = sys::CVPixelBufferUnlockBaseAddress(self.0, 0);
            if status != 0 {
                tracing::error!("CVPixelBufferUnlockBaseAddress failed: status={status}");
            }
        }
    }
}

// ドロップ時に確実に sys::CFRelease() を呼び出すようにするためのラッパー
#[derive(Debug)]
pub(crate) struct CfPtrMut<T>(pub(crate) *mut T);

impl<T> Drop for CfPtrMut<T> {
    fn drop(&mut self) {
        unsafe { sys::CFRelease(self.0.cast()) }
    }
}

#[derive(Debug)]
pub(crate) struct CfPtr<T>(pub(crate) *const T);

impl<T> Drop for CfPtr<T> {
    fn drop(&mut self) {
        unsafe { sys::CFRelease(self.0.cast()) }
    }
}

pub(crate) fn cf_dictionary(
    kvs: &[(sys::CFStringRef, *const c_void)],
) -> Result<sys::CFDictionaryRef, Error> {
    let mut keys = kvs.iter().map(|(k, _)| k.cast()).collect::<Vec<_>>();
    let mut values = kvs.iter().map(|(_, v)| *v).collect::<Vec<_>>();
    let ptr = unsafe {
        sys::CFDictionaryCreate(
            std::ptr::null_mut(),
            keys.as_mut_ptr(),
            values.as_mut_ptr(),
            kvs.len() as sys::CFIndex,
            &sys::kCFTypeDictionaryKeyCallBacks,
            &sys::kCFTypeDictionaryValueCallBacks,
        )
    };
    if ptr.is_null() {
        return Err(Error::CfObjectCreationFailed {
            function: "CFDictionaryCreate".into(),
        });
    }
    Ok(ptr)
}

/// CF オブジェクトの配列から CFArray を生成する
///
/// `kCFTypeArrayCallBacks` を使うため、生成された CFArray は各要素を retain する。
/// 呼び出し側は渡した要素の `CfPtr` を CFArray 生成後に drop してよい。
pub(crate) fn cf_array(values: &[*const c_void]) -> Result<CfPtr<c_void>, Error> {
    let ptr = unsafe {
        sys::CFArrayCreate(
            std::ptr::null_mut(),
            values.as_ptr().cast_mut(),
            values.len() as sys::CFIndex,
            &sys::kCFTypeArrayCallBacks,
        )
    };
    if ptr.is_null() {
        return Err(Error::CfObjectCreationFailed {
            function: "CFArrayCreate".into(),
        });
    }
    Ok(CfPtr(ptr.cast()))
}

pub(crate) fn cf_number_i32(n: i32) -> Result<CfPtr<c_void>, Error> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberSInt32Type as sys::CFNumberType,
            ((&n) as *const i32).cast(),
        )
    };
    if ptr.is_null() {
        return Err(Error::CfObjectCreationFailed {
            function: "CFNumberCreate".into(),
        });
    }
    Ok(CfPtr(ptr.cast()))
}

pub(crate) fn cf_number_i64(n: i64) -> Result<CfPtr<c_void>, Error> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberSInt64Type as sys::CFNumberType,
            ((&n) as *const i64).cast(),
        )
    };
    if ptr.is_null() {
        return Err(Error::CfObjectCreationFailed {
            function: "CFNumberCreate".into(),
        });
    }
    Ok(CfPtr(ptr.cast()))
}

pub(crate) fn cf_number_f64(n: f64) -> Result<CfPtr<c_void>, Error> {
    let ptr = unsafe {
        sys::CFNumberCreate(
            std::ptr::null_mut(),
            sys::kCFNumberFloat64Type as sys::CFNumberType,
            ((&n) as *const f64).cast(),
        )
    };
    if ptr.is_null() {
        return Err(Error::CfObjectCreationFailed {
            function: "CFNumberCreate".into(),
        });
    }
    Ok(CfPtr(ptr.cast()))
}
