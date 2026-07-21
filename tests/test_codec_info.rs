//! `src/codec_info.rs` に対応する単体テスト

use shiguredo_video_toolbox::{VideoCodecType, supported_codecs};

#[test]
fn test_supported_codecs() {
    // README の動作要件（macOS / arm64）および CI のセルフホスト（`macOS` / `ARM64`）と整合する前提。
    // Intel Mac のローカル等では H.264/HEVC の assert が失敗しうる（README の「テスト」セクションを参照）。
    let codecs = supported_codecs();

    // 4 種類のコーデックが返る
    assert_eq!(codecs.len(), 4);

    // H.264 デコード・エンコード（上記前提の環境ではハードウェア対応を期待）
    let h264 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::H264)
        .expect("H.264 のコーデック情報が返ってくること");
    assert!(h264.decoding.supported);
    assert!(h264.encoding.supported);

    // HEVC デコード・エンコード（上記前提の環境ではハードウェア対応を期待）
    let hevc = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Hevc)
        .expect("HEVC のコーデック情報が返ってくること");
    assert!(hevc.decoding.supported);
    assert!(hevc.encoding.supported);

    // VP9 エンコードは VideoToolbox ではサポートされていない
    let vp9 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Vp9)
        .expect("VP9 のコーデック情報が返ってくること");
    assert!(!vp9.encoding.supported);

    // AV1 エンコードは VideoToolbox ではサポートされていない
    let av1 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Av1)
        .expect("AV1 のコーデック情報が返ってくること");
    assert!(!av1.encoding.supported);
}
