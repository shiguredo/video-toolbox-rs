//! `src/error.rs` に対応する単体テスト

use shiguredo_video_toolbox::Error;

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
