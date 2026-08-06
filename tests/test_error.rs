//! `src/error.rs` に対応する単体テスト

use shiguredo_video_toolbox::{Error, PixelFormat};

/// Error::LimitExceeded と Error::CfObjectCreationFailed の Display 出力が、
/// reason / function を含む英語メッセージになることを検証する
#[test]
fn error_display_limit_exceeded_and_cf_object_creation_failed() {
    let e = Error::LimitExceeded {
        reason: "unit test reason".into(),
    };
    assert!(e.to_string().contains("limit exceeded"));
    assert!(e.to_string().contains("unit test reason"));

    let e2 = Error::CfObjectCreationFailed {
        function: "CFNumberCreate".into(),
    };
    let s = e2.to_string();
    assert!(s.contains("CFNumberCreate"));
    assert!(s.contains("null"));
}

/// Error::UnknownPixelFormat の Display 出力が、FourCC と期待フォーマットを含む
/// 英語メッセージになることを検証する。FourCC 0x30323449 は 'I420' の
/// リトルエンディアン表現で、Nv12 期待時に I420 バッファを渡した状況を再現する
#[test]
fn error_display_unknown_pixel_format() {
    let e = Error::UnknownPixelFormat {
        expected: PixelFormat::Nv12,
        fourcc: 0x3032_3449,
    };
    let s = e.to_string();
    assert!(s.contains("0x30323449"));
    assert!(s.contains("Nv12"));
}
