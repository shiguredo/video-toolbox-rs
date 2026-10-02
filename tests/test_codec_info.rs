//! `src/codec_info.rs` に対応する単体テスト

use shiguredo_video_toolbox::{
    EncodingProfiles, H264EncodingProfile, HevcEncodingProfile, VideoCodecType, supported_codecs,
};

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

/// ハードウェアデコード可否とデコードの `hardware_accelerated` が一致することを検証する
///
/// `DecodingInfo` の rustdoc にある「`hardware_accelerated` は現状 `supported` と同じ値になる」
/// という契約を固定する。ソフトウェアデコードのみの環境では両方 false になるため、
/// 値そのものではなく一致だけを検証する。
#[test]
fn test_decoding_info_hardware_accelerated_matches_supported() {
    for info in supported_codecs() {
        assert_eq!(
            info.decoding.hardware_accelerated, info.decoding.supported,
            "{:?} の hardware_accelerated が supported と一致しない",
            info.codec
        );
    }
}

/// H.264 / HEVC のエンコードプロファイル照会が、対応するバリアントで
/// 空でないプロファイル一覧を返すことを検証する
///
/// `VTCopySupportedPropertyDictionaryForEncoder` の結果を CFString と突き合わせる経路
/// (`query_encoding_profiles` / `match_profiles`) を通す。プロファイルの対応表は
/// macOS のバージョンで増減しうるため、個々の値ではなく「代表的なプロファイルが
/// 含まれること」と「H.264 / HEVC 以外は None になること」を検証する。
#[test]
fn test_encoding_profiles() {
    let codecs = supported_codecs();

    let h264 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::H264)
        .expect("H.264 のコーデック情報が返ってくること");
    match &h264.encoding.profiles {
        EncodingProfiles::H264(profiles) => {
            assert!(!profiles.is_empty(), "H.264 のプロファイルが空になっている");
            // プロファイル照会は 1920x1080 を代表値として行うため、
            // 一般的な環境では Main と High の両方が列挙される
            assert!(
                profiles.contains(&H264EncodingProfile::Main),
                "H.264 のプロファイルに Main が含まれない: {profiles:?}"
            );
            assert!(
                profiles.contains(&H264EncodingProfile::High),
                "H.264 のプロファイルに High が含まれない: {profiles:?}"
            );
        }
        other => panic!("H.264 のプロファイルが H264 バリアントで返っていない: {other:?}"),
    }

    let hevc = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Hevc)
        .expect("HEVC のコーデック情報が返ってくること");
    match &hevc.encoding.profiles {
        EncodingProfiles::Hevc(profiles) => {
            assert!(!profiles.is_empty(), "HEVC のプロファイルが空になっている");
            assert!(
                profiles.contains(&HevcEncodingProfile::Main),
                "HEVC のプロファイルに Main が含まれない: {profiles:?}"
            );
        }
        other => panic!("HEVC のプロファイルが Hevc バリアントで返っていない: {other:?}"),
    }

    // エンコード非対応のコーデックではプロファイル情報を返さない
    for codec in [VideoCodecType::Vp9, VideoCodecType::Av1] {
        let info = codecs
            .iter()
            .find(|c| c.codec == codec)
            .expect("コーデック情報が返ってくること");
        assert!(
            matches!(info.encoding.profiles, EncodingProfiles::None),
            "{codec:?} のプロファイルが None になっていない: {:?}",
            info.encoding.profiles
        );
    }
}

/// `EncodingInfo` のケーパビリティが、H.264 / HEVC ではエンコード対応と整合し、
/// VP9 / AV1 ではすべて false になることを検証する
#[test]
fn test_encoding_capabilities_are_consistent() {
    for info in supported_codecs() {
        if !info.encoding.supported {
            assert!(
                !info.encoding.hardware_accelerated,
                "{:?} はエンコード非対応なのに hardware_accelerated が true",
                info.codec
            );
            assert!(
                !info.encoding.supports_frame_reordering,
                "{:?} はエンコード非対応なのに supports_frame_reordering が true",
                info.codec
            );
            assert!(
                !info.encoding.supports_multi_pass,
                "{:?} はエンコード非対応なのに supports_multi_pass が true",
                info.codec
            );
        }
    }

    // 前提環境では H.264 のエンコーダーはハードウェアアクセラレーションに対応する
    let h264 = supported_codecs()
        .into_iter()
        .find(|c| c.codec == VideoCodecType::H264)
        .expect("H.264 のコーデック情報が返ってくること");
    assert!(
        h264.encoding.hardware_accelerated,
        "H.264 のエンコードがハードウェアアクセラレーションに対応していない"
    );
}
