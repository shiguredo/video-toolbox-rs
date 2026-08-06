use crate::types::PixelFormat;

/// エラー
#[derive(Debug)]
pub enum Error {
    /// Video Toolbox API のエラー
    VideoToolbox {
        /// ステータスコード
        status: i32,
        /// 関数名
        function: String,
    },
    /// ピクセルフォーマットの不一致
    PixelFormatMismatch {
        /// 期待するピクセルフォーマット
        expected: PixelFormat,
        /// 実際のピクセルフォーマット
        actual: PixelFormat,
    },
    /// フレームデータのサイズ不足
    InsufficientFrameData {
        /// プレーン名
        plane: String,
        /// 期待する最小サイズ
        expected: usize,
        /// 実際のサイズ
        actual: usize,
    },
    /// コーデックが未対応
    UnsupportedCodec {
        /// コーデック名
        codec: String,
    },
    /// 不正な設定値
    InvalidConfig {
        /// フィールド名
        field: String,
        /// 理由
        reason: String,
    },
    /// 防御的な上限超過や異常値（null ポインタ、PTS の加算オーバーフロー等）
    LimitExceeded {
        /// 英語の理由（表示用）
        reason: String,
    },
    /// Core Foundation のオブジェクト生成が NULL を返した（メモリ不足等）
    CfObjectCreationFailed {
        /// 関数名
        function: String,
    },
    /// 認識できないピクセルフォーマット (I420/Nv12 のいずれでもない FourCC が渡された)
    UnknownPixelFormat {
        /// 期待するピクセルフォーマット
        expected: PixelFormat,
        /// 実際の FourCC
        fourcc: u32,
    },
}

impl Error {
    pub(crate) fn check(status: i32, function: impl Into<String>) -> Result<(), Self> {
        if status == 0 {
            return Ok(());
        }
        Err(Self::VideoToolbox {
            status,
            function: function.into(),
        })
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VideoToolbox { status, function } => {
                write!(
                    f,
                    "[{}] {}() failed: status={}",
                    env!("CARGO_PKG_NAME"),
                    function,
                    status
                )
            }
            Self::PixelFormatMismatch { expected, actual } => {
                write!(
                    f,
                    "pixel format mismatch: encoder expects {expected:?}, but got {actual:?}"
                )
            }
            Self::InsufficientFrameData {
                plane,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "insufficient frame data for {plane} plane: expected at least {expected} bytes, but got {actual}"
                )
            }
            Self::UnsupportedCodec { codec } => {
                write!(f, "codec {codec} is not supported on this platform")
            }
            Self::InvalidConfig { field, reason } => {
                write!(f, "invalid config: {field}: {reason}")
            }
            Self::LimitExceeded { reason } => {
                write!(f, "limit exceeded: {reason}")
            }
            Self::CfObjectCreationFailed { function } => {
                write!(
                    f,
                    "Core Foundation object creation failed: {}() returned null",
                    function
                )
            }
            Self::UnknownPixelFormat { expected, fourcc } => {
                write!(
                    f,
                    "unknown pixel format: encoder expects {expected:?}, but got FourCC=0x{fourcc:08x}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}
