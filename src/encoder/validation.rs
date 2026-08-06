//! エンコーダー設定の検証

use crate::{
    encoder::{
        Encoder,
        config::{DataRateLimit, EncoderConfig, ReconfigureParams},
        handler::EncodeHandler,
    },
    error::Error,
    types::validate_video_dimensions_for_toolbox,
};

/// `average_bitrate` (bps) の境界を検証する
fn validate_average_bitrate(bitrate: u64) -> Result<(), Error> {
    if bitrate == 0 {
        return Err(Error::InvalidConfig {
            field: "average_bitrate".into(),
            reason: "must not be zero".into(),
        });
    }
    if bitrate > i64::MAX as u64 {
        return Err(Error::InvalidConfig {
            field: "average_bitrate".into(),
            reason: "must fit in i64 for CFNumber".into(),
        });
    }
    Ok(())
}

/// `data_rate_limits` の境界を検証する
///
/// kVTCompressionPropertyKey_DataRateLimits は「0〜2 個のハードリミット」と規定されている
/// (VTCompressionProperties.h)。
fn validate_data_rate_limits(limits: &[DataRateLimit]) -> Result<(), Error> {
    if limits.len() > 2 {
        return Err(Error::InvalidConfig {
            field: "data_rate_limits".into(),
            reason: "must contain at most two limits".into(),
        });
    }
    for limit in limits {
        if limit.bytes == 0 {
            return Err(Error::InvalidConfig {
                field: "data_rate_limits".into(),
                reason: "bytes must not be zero".into(),
            });
        }
        if limit.bytes > i64::MAX as u64 {
            return Err(Error::InvalidConfig {
                field: "data_rate_limits".into(),
                reason: "bytes must fit in i64 for CFNumber".into(),
            });
        }
        if limit.window.is_zero() {
            return Err(Error::InvalidConfig {
                field: "data_rate_limits".into(),
                reason: "window must not be zero".into(),
            });
        }
    }
    Ok(())
}

/// `field` で示す設定値の境界を検証する
///
/// `u32` 値のゼロ拒否 (reason は `"must not be zero"` に固定) と `i32::MAX` 上限拒否を行う。
/// 上限超過時の `reason_overflow` は呼び出し側が用途に応じて指定する
/// (CMTimeMake の timescale 用 / CFNumber 用など)。i32 上限が不要なフィールド
/// (例: `fps_denominator`) には使わない。
fn validate_positive_i32_field(
    field: &'static str,
    reason_overflow: &'static str,
    value: u32,
) -> Result<(), Error> {
    if value == 0 {
        return Err(Error::InvalidConfig {
            field: field.into(),
            reason: "must not be zero".into(),
        });
    }
    if value > i32::MAX as u32 {
        return Err(Error::InvalidConfig {
            field: field.into(),
            reason: reason_overflow.into(),
        });
    }
    Ok(())
}

impl<H: EncodeHandler> Encoder<H> {
    /// エンコーダー設定を検証する
    pub(super) fn validate_config(config: &EncoderConfig) -> Result<(), Error> {
        validate_video_dimensions_for_toolbox(config.width, config.height)?;
        // fps_denominator は CMTimeMake の timescale には使われず、div_ceil の除数と
        // PTS 計算の加数としてのみ使われるため、i32 上限は不要 (ゼロ拒否のみ)
        if config.fps_denominator == 0 {
            return Err(Error::InvalidConfig {
                field: "fps_denominator".into(),
                reason: "must not be zero".into(),
            });
        }
        validate_positive_i32_field(
            "fps_numerator",
            "must fit in i32 for CMTime timescale",
            config.fps_numerator,
        )?;
        if let Some(bitrate) = config.average_bitrate {
            validate_average_bitrate(bitrate)?;
        }
        if let Some(limits) = &config.data_rate_limits {
            validate_data_rate_limits(limits)?;
        }
        // NonZeroU32 から i32 へのキャストで負値に切り詰まるのを防ぐ
        if let Some(interval) = config.max_key_frame_interval
            && interval.get() > i32::MAX as u32
        {
            return Err(Error::InvalidConfig {
                field: "max_key_frame_interval".into(),
                reason: "must fit in i32 for CFNumber".into(),
            });
        }
        if let Some(delay_count) = config.max_frame_delay_count
            && delay_count.get() > i32::MAX as u32
        {
            return Err(Error::InvalidConfig {
                field: "max_frame_delay_count".into(),
                reason: "must fit in i32 for CFNumber".into(),
            });
        }
        Ok(())
    }

    /// [`crate::encoder::Encoder::reconfigure`] に渡された [`ReconfigureParams`] を検証する
    pub(super) fn validate_reconfigure_params(params: &ReconfigureParams) -> Result<(), Error> {
        if let Some(bitrate) = params.average_bitrate {
            validate_average_bitrate(bitrate)?;
        }
        if let Some(fps) = params.expected_frame_rate {
            validate_positive_i32_field(
                "expected_frame_rate",
                "must fit in i32 for CFNumber",
                fps,
            )?;
        }
        if let Some(ref limits) = params.data_rate_limits {
            validate_data_rate_limits(limits)?;
        }
        Ok(())
    }
}
