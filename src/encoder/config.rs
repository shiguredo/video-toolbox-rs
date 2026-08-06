//! エンコーダーの設定型 (プロファイル・コーデック設定・設定構造体)

use std::{num::NonZeroU32, time::Duration};

use crate::types::PixelFormat;

/// H.264 プロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Profile {
    /// Baseline (最高速)
    Baseline,
    /// Main
    Main,
    /// High (高品質)
    High,
}

/// H.264 エントロピー符号化モード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264EntropyMode {
    /// CAVLC (高速)
    Cavlc,
    /// CABAC (高品質)
    Cabac,
}

/// H.264 エンコーダー固有の設定
#[derive(Debug, Clone)]
pub struct H264EncoderConfig {
    /// kVTCompressionPropertyKey_ProfileLevel
    pub profile: H264Profile,
    /// kVTCompressionPropertyKey_H264EntropyMode
    pub entropy_mode: H264EntropyMode,
}

/// HEVC プロファイル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HevcProfile {
    /// Main
    Main,
    /// Main10
    Main10,
}

/// HEVC エンコーダー固有の設定
#[derive(Debug, Clone)]
pub struct HevcEncoderConfig {
    /// kVTCompressionPropertyKey_ProfileLevel
    pub profile: HevcProfile,
    /// kVTCompressionPropertyKey_AllowOpenGOP
    pub allow_open_gop: bool,
}

/// コーデック固有の設定
#[derive(Debug, Clone)]
pub enum CodecConfig {
    /// H.264
    H264(H264EncoderConfig),
    /// HEVC (H.265)
    Hevc(HevcEncoderConfig),
}

/// エンコーダーに指定する設定
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// VTCompressionSessionCreate の width 引数
    pub width: u32,

    /// VTCompressionSessionCreate の height 引数
    pub height: u32,

    /// VTCompressionSessionCreate の codecType 引数およびコーデック固有の設定
    pub codec: CodecConfig,

    /// 入力ピクセルフォーマット
    ///
    /// Video Toolbox は CVPixelBuffer 単位でピクセルフォーマットを持つため、
    /// API レベルではフレームごとに異なるフォーマットを渡すことが可能である。
    /// しかし、エンコードセッション中にフォーマットが混在する使い方は一般的ではないため、
    /// このライブラリではセッション全体でフォーマットを固定し、
    /// `Encoder::encode()` で `FrameData` のバリアントとの整合性を検証する。
    pub pixel_format: PixelFormat,

    /// kVTCompressionPropertyKey_AverageBitRate (bps 単位、未指定時はバックエンド依存)
    pub average_bitrate: Option<u64>,

    /// kVTCompressionPropertyKey_ExpectedFrameRate の分子 (CMTimeMake の timescale としても使用)
    pub fps_numerator: u32,

    /// kVTCompressionPropertyKey_ExpectedFrameRate の分母 (CMTimeMake のフレーム間隔としても使用)
    pub fps_denominator: u32,

    /// kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality
    pub prioritize_encoding_speed_over_quality: bool,

    /// kVTCompressionPropertyKey_RealTime
    pub real_time: bool,

    /// kVTCompressionPropertyKey_MaximizePowerEfficiency
    pub maximize_power_efficiency: bool,

    /// kVTCompressionPropertyKey_AllowFrameReordering
    pub allow_frame_reordering: bool,

    /// kVTCompressionPropertyKey_AllowTemporalCompression
    pub allow_temporal_compression: bool,

    /// kVTCompressionPropertyKey_MaxKeyFrameInterval
    pub max_key_frame_interval: Option<NonZeroU32>,

    /// kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration
    pub max_key_frame_interval_duration: Option<Duration>,

    /// kVTCompressionPropertyKey_MaxFrameDelayCount
    pub max_frame_delay_count: Option<NonZeroU32>,

    /// kVTCompressionPropertyKey_DataRateLimits
    ///
    /// `None` は未設定。`Some(空 Vec)` も未設定と同じ扱いで、[`crate::encoder::Encoder::new`] 時に `None` へ
    /// 正規化される ([`crate::encoder::Encoder::config`] が返す表現を一意にするため)。
    /// 詳細は [`DataRateLimit`] を参照。
    pub data_rate_limits: Option<Vec<DataRateLimit>>,
}

/// データレートのハードリミット 1 個分
///
/// `window` 秒間の任意の連続区間で、圧縮データの総量が `bytes` を超えないことを
/// エンコーダーに要求する (kVTCompressionPropertyKey_DataRateLimits)。
///
/// VTCompressionProperties.h の discussion は、`AverageBitRate` で全体の目標を指定しつつ
/// 本プロパティで短期ウィンドウのハード上限を併設する使い方を推奨している。
/// 指定できるリミットは Video Toolbox の仕様上 0〜2 個で、コーデックによっては
/// 指定レートに収まらないことがある (仕様は将来変更される可能性がある)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRateLimit {
    /// ウィンドウあたりの総バイト数の上限
    pub bytes: u64,
    /// ウィンドウの長さ
    pub window: Duration,
}

/// VTCompressionSessionEncodeFrame の frameProperties に指定するオプション
#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    /// kVTEncodeFrameOptionKey_ForceKeyFrame
    pub force_key_frame: bool,
}

/// [`crate::encoder::Encoder::reconfigure`] で動的に更新可能なエンコードパラメータ
///
/// `None` のフィールドは現在値を維持する。全項目 `None` の場合は no-op となる。
/// 動的に更新できない項目の扱いを含む詳細は [`crate::encoder::Encoder::reconfigure`] の rustdoc を参照。
///
/// `#[non_exhaustive]` は付けていないため、フィールド追加は破壊的変更 (メジャーバージョンアップ) となる。
/// 構築時は `ReconfigureParams::default()` を起点に必要なフィールドだけ更新すると、
/// フィールド追加時の修正箇所を減らせる。
#[derive(Debug, Clone, Default)]
pub struct ReconfigureParams {
    /// kVTCompressionPropertyKey_AverageBitRate (bps 単位)
    pub average_bitrate: Option<u64>,

    /// kVTCompressionPropertyKey_ExpectedFrameRate (整数 fps)
    ///
    /// 詳細な正規化 / 再スケール挙動は [`crate::encoder::Encoder::reconfigure`] の rustdoc を参照。
    pub expected_frame_rate: Option<u32>,

    /// kVTCompressionPropertyKey_DataRateLimits
    ///
    /// `None` は現在値を維持する。`Some(空 Vec)` は設定済みの上限を解除する。
    /// 詳細は [`DataRateLimit`] を参照。
    pub data_rate_limits: Option<Vec<DataRateLimit>>,
}
