//! `src/error.rs` に対応する単体テスト

use shiguredo_video_toolbox::{Error, PixelFormat};

#[test]
fn error_display_limit_exceeded_and_cf_object_creation_failed() {
    let e = Error::LimitExceeded {
        reason: "unit test reason",
    };
    assert!(e.to_string().contains("limit exceeded"));
    assert!(e.to_string().contains("unit test reason"));

    let e2 = Error::CfObjectCreationFailed {
        function: "CFNumberCreate",
    };
    let s = e2.to_string();
    assert!(s.contains("CFNumberCreate"));
    assert!(s.contains("null"));
}

#[test]
fn error_display_unknown_pixel_format() {
    // FourCC `I420` (0x30323449) を未知フォーマットとして渡した場合の表示を確認する
    let e = Error::UnknownPixelFormat {
        expected: PixelFormat::Nv12,
        fourcc: 0x3032_3449,
    };
    let s = e.to_string();
    assert!(s.contains("0x30323449"));
    assert!(s.contains("Nv12"));
}
