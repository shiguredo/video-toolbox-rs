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

/// Error::VideoToolbox の Display 出力が、関数名とステータスコードを含む英語メッセージになることを検証する
///
/// プロパティを指定しない呼び出し (`property: None`) では、関数名の後ろに空の `()` が付く。
#[test]
fn error_display_video_toolbox() {
    let e = Error::VideoToolbox {
        status: -12_345,
        function: "VTCompressionSessionCreate".into(),
        property: None,
    };
    let s = e.to_string();
    assert!(
        s.contains("VTCompressionSessionCreate()"),
        "実際の出力: {s}"
    );
    assert!(s.contains("status=-12345"), "実際の出力: {s}");
    assert!(
        s.contains(env!("CARGO_PKG_NAME")),
        "クレート名が含まれること: {s}"
    );
}

/// Error::VideoToolbox の Display 出力が、受け付けられなかったプロパティ名を含むことを検証する
///
/// Video Toolbox がプロパティの設定を受け付けなかった場合に、どのプロパティが原因かを
/// メッセージから特定できる必要がある。
#[test]
fn error_display_video_toolbox_property() {
    let e = Error::VideoToolbox {
        status: -12_900,
        function: "VTSessionSetProperty".into(),
        property: Some("kVTCompressionPropertyKey_MaxFrameDelayCount".into()),
    };
    let s = e.to_string();
    assert!(
        s.contains("VTSessionSetProperty(kVTCompressionPropertyKey_MaxFrameDelayCount)"),
        "実際の出力: {s}"
    );
    assert!(s.contains("status=-12900"), "実際の出力: {s}");
}

/// Error::PixelFormatMismatch の Display 出力が、期待フォーマットと実際のフォーマットを含む
/// 英語メッセージになることを検証する
#[test]
fn error_display_pixel_format_mismatch() {
    let e = Error::PixelFormatMismatch {
        expected: PixelFormat::I420,
        actual: PixelFormat::Nv12,
    };
    let s = e.to_string();
    assert!(s.contains("pixel format mismatch"), "実際の出力: {s}");
    assert!(s.contains("I420"), "実際の出力: {s}");
    assert!(s.contains("Nv12"), "実際の出力: {s}");
}

/// Error::InsufficientFrameData の Display 出力が、プレーン名と期待サイズ・実際のサイズを
/// 含む英語メッセージになることを検証する
#[test]
fn error_display_insufficient_frame_data() {
    let e = Error::InsufficientFrameData {
        plane: "Y".into(),
        expected: 307_200,
        actual: 1,
    };
    let s = e.to_string();
    assert!(s.contains("Y plane"), "実際の出力: {s}");
    assert!(s.contains("at least 307200 bytes"), "実際の出力: {s}");
    assert!(s.contains("but got 1"), "実際の出力: {s}");
}

/// Error::UnsupportedCodec の Display 出力が、コーデック名を含む英語メッセージになることを検証する
#[test]
fn error_display_unsupported_codec() {
    let e = Error::UnsupportedCodec {
        codec: "VP9".into(),
    };
    let s = e.to_string();
    assert!(s.contains("VP9"), "実際の出力: {s}");
    assert!(s.contains("not supported"), "実際の出力: {s}");
}

/// Error::InvalidConfig の Display 出力が、フィールド名と理由を含む英語メッセージになることを検証する
#[test]
fn error_display_invalid_config() {
    let e = Error::InvalidConfig {
        field: "width".into(),
        reason: "must not be zero".into(),
    };
    let s = e.to_string();
    assert!(s.contains("invalid config"), "実際の出力: {s}");
    assert!(s.contains("width"), "実際の出力: {s}");
    assert!(s.contains("must not be zero"), "実際の出力: {s}");
}

/// Error が std::error::Error を実装していることを検証する
///
/// `Box<dyn std::error::Error>` へ変換できることと、`source()` が None を返すことを確認する。
#[test]
fn error_implements_std_error() {
    use std::error::Error as StdError;

    let e = Error::UnsupportedCodec {
        codec: "AV1".into(),
    };
    let source = StdError::source(&e);
    assert!(source.is_none(), "source() は None を返すこと");

    let boxed: Box<dyn StdError> = Box::new(e);
    assert!(boxed.to_string().contains("AV1"));
}
