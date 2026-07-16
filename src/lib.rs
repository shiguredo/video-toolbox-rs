//! [Hisui] 用の [Video Toolbox] エンコーダーおよびデコーダー
//!
//! [Hisui]: https://github.com/shiguredo/hisui
//! [Video Toolbox]: https://developer.apple.com/documentation/videotoolbox/
#![warn(missing_docs)]

// macOS 以外ではビルドを許可しない (cargo doc 時は除外)
#[cfg(all(not(target_os = "macos"), not(doc)))]
compile_error!("this crate only supports macOS");

mod codec_info;
mod decoder;
mod encoder;
mod error;
mod sys;
mod types;

#[cfg(target_os = "macos")]
pub use codec_info::supported_codecs;
pub use codec_info::{
    CodecInfo, DecodingInfo, EncodingInfo, EncodingProfiles, H264EncodingProfile,
    HevcEncodingProfile, VideoCodecType,
};
pub use decoder::{
    DecodeHandler, DecodedFrame, Decoder, DecoderCodec, DecoderConfig, FnDecodeHandler, I420Frame,
    Nv12Frame,
};
pub use encoder::{
    CodecConfig, DataRateLimit, EncodeHandler, EncodeOptions, EncodedFrame, Encoder, EncoderConfig,
    FnEncodeHandler, FrameData, H264EncoderConfig, H264EntropyMode, H264Profile, HevcEncoderConfig,
    HevcProfile, ReconfigureParams,
};
pub use error::Error;
pub use types::PixelFormat;
