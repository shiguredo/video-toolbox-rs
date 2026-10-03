//! `src/codec_info.rs` に対応する単体テスト

use shiguredo_video_toolbox::{
    EncodingProfiles, H264EncodingProfile, HevcEncodingProfile, VideoCodecType,
    query_encoding_capabilities, supported_codecs,
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
    assert!(h264.decoding.hardware_accelerated);
    assert!(!h264.encoders.is_empty());

    // HEVC デコード・エンコード（上記前提の環境ではハードウェア対応を期待）
    let hevc = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Hevc)
        .expect("HEVC のコーデック情報が返ってくること");
    assert!(hevc.decoding.hardware_accelerated);
    assert!(!hevc.encoders.is_empty());

    // VP9 エンコードは VideoToolbox ではサポートされていない
    let vp9 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Vp9)
        .expect("VP9 のコーデック情報が返ってくること");
    assert!(vp9.encoders.is_empty());

    // AV1 エンコードは VideoToolbox ではサポートされていない
    let av1 = codecs
        .iter()
        .find(|c| c.codec == VideoCodecType::Av1)
        .expect("AV1 のコーデック情報が返ってくること");
    assert!(av1.encoders.is_empty());
}

/// エンコーダー一覧が、H.264 / HEVC ではハードウェアとソフトウェアのエンコーダーを含み、
/// VP9 / AV1 では空になることを検証する
///
/// `EncodingInfo` はエンコーダー 1 件分の情報であり、解像度に依存しない。
/// 解像度ごとにどのエンコーダーが選ばれるかは `query_encoding_capabilities` のテストで扱う。
#[test]
fn test_encoder_infos_are_listed_per_codec() {
    for info in supported_codecs() {
        for encoder in &info.encoders {
            assert!(
                !encoder.encoder_id.is_empty(),
                "{:?} のエンコーダー ID が空になっている",
                info.codec
            );

            // `kVTVideoEncoderList_SupportsFrameReordering` はキーが無い場合に true と見なす
            // 仕様なので、このキーを設定しない Apple 製エンコーダーでは true になる。
            // macOS がこのキーを設定するようになり値が変わった場合は、この assert を見直すこと。
            assert!(
                encoder.supports_frame_reordering,
                "{:?} の {} がフレームリオーダリングに対応していない",
                info.codec, encoder.encoder_id
            );

            // `kVTVideoEncoderList_SupportsMultiPass` はキーが無い場合に false と見なす仕様であり、
            // このキーは macOS では利用できない（API_UNAVAILABLE）ため常に false になる
            assert!(
                !encoder.supports_multi_pass,
                "{:?} の {} がマルチパスエンコードに対応している",
                info.codec, encoder.encoder_id
            );
        }
    }

    for codec in [VideoCodecType::H264, VideoCodecType::Hevc] {
        let info = supported_codecs()
            .into_iter()
            .find(|c| c.codec == codec)
            .expect("コーデック情報が返ってくること");

        assert!(
            !info.encoders.is_empty(),
            "{codec:?} のエンコーダーが 1 つも返っていない"
        );

        // 前提環境では H.264 / HEVC のハードウェアエンコーダーが存在する
        assert!(
            info.encoders.iter().any(|e| e.hardware_accelerated),
            "{codec:?} のハードウェアエンコーダーが一覧に無い"
        );

        // ハードウェアエンコーダーが対応しない解像度で使われるソフトウェアエンコーダーも一覧に含まれる
        assert!(
            info.encoders.iter().any(|e| !e.hardware_accelerated),
            "{codec:?} のソフトウェアエンコーダーが一覧に無い"
        );
    }

    for codec in [VideoCodecType::Vp9, VideoCodecType::Av1] {
        let info = supported_codecs()
            .into_iter()
            .find(|c| c.codec == codec)
            .expect("コーデック情報が返ってくること");

        assert!(
            info.encoders.is_empty(),
            "{codec:?} のエンコーダーが返っている: {:?}",
            info.encoders
        );
    }
}

/// 解像度を指定した照会が、選択されるエンコーダーとプロファイルを返すことを検証する
///
/// 1920x1080 は README の動作要件（macOS / arm64）を満たす環境で、H.264 / HEVC ともに
/// ハードウェアエンコーダーが使われる解像度である。
/// `VTCopySupportedPropertyDictionaryForEncoder` が返す `encoderIDOut` と、
/// その ID に一致する `VTCopyVideoEncoderList` のエントリを突き合わせる経路
/// (`find_encoder_info`) を通す。
#[test]
fn test_query_encoding_capabilities_encoder_identity() {
    for codec in [VideoCodecType::H264, VideoCodecType::Hevc] {
        let capabilities =
            query_encoding_capabilities(codec, 1920, 1080).expect("エンコーダーが取得できること");

        assert!(
            capabilities.encoder.hardware_accelerated,
            "{codec:?} の 1920x1080 でハードウェアエンコーダーが使えない"
        );

        let encoder = &capabilities.encoder;
        assert!(
            !encoder.encoder_id.is_empty(),
            "{codec:?} のエンコーダー ID が空になっている"
        );
        assert!(
            encoder
                .encoder_name
                .as_deref()
                .is_some_and(|name| !name.is_empty()),
            "{codec:?} のエンコーダー表示名が取得できていない: {encoder:?}"
        );
        assert!(
            encoder
                .codec_name
                .as_deref()
                .is_some_and(|name| !name.is_empty()),
            "{codec:?} のコーデック表示名が取得できていない: {encoder:?}"
        );

        // 性能・品質指標は optional なキーなので、取得できた場合は有限の数値であることを検証する
        for rating in [encoder.performance_rating, encoder.quality_rating]
            .into_iter()
            .flatten()
        {
            assert!(
                rating.is_finite(),
                "{codec:?} の指標が有限の数値でない: {rating}"
            );
        }
        // `CFNumber` から `f64` を取り出す経路 (`get_cf_number_f64`) が常に `None` を
        // 返していても上の検証は通ってしまうため、前提環境では性能指標が取得できることだけは固定する
        assert!(
            encoder.performance_rating.is_some(),
            "{codec:?} の性能指標が取得できていない: {encoder:?}"
        );

        // プロファイルは同じサポートプロパティ辞書から読み出す
        match (codec, capabilities.profiles.as_ref()) {
            (VideoCodecType::H264, Some(EncodingProfiles::H264(profiles))) => {
                assert!(!profiles.is_empty(), "H.264 のプロファイルが空になっている");
                for profile in [H264EncodingProfile::Main, H264EncodingProfile::High] {
                    assert!(
                        profiles.contains(&profile),
                        "H.264 のプロファイルに {profile:?} が含まれない: {profiles:?}"
                    );
                }
            }
            (VideoCodecType::Hevc, Some(EncodingProfiles::Hevc(profiles))) => {
                assert!(!profiles.is_empty(), "HEVC のプロファイルが空になっている");
                assert!(
                    profiles.contains(&HevcEncodingProfile::Main),
                    "HEVC のプロファイルに Main が含まれない: {profiles:?}"
                );
            }
            (codec, profiles) => {
                panic!("{codec:?} のプロファイルが対応するバリアントで返っていない: {profiles:?}")
            }
        }
    }
}

/// 解像度を指定した照会が返すエンコーダーが、コーデックのエンコーダー一覧の要素であることを検証する
///
/// `EncodingCapabilities::encoder` は `CodecInfo::encoders` のいずれかであるという契約を固定する。
/// 一覧に無いエンコーダーが返ると、利用側は一覧と突き合わせて属性を引けなくなる。
#[test]
fn test_query_encoding_capabilities_returns_listed_encoder() {
    for codec in [VideoCodecType::H264, VideoCodecType::Hevc] {
        let codec_info = supported_codecs()
            .into_iter()
            .find(|c| c.codec == codec)
            .expect("コーデック情報が返ってくること");

        for (width, height) in [(1920, 1080), (16384, 16384)] {
            let capabilities = query_encoding_capabilities(codec, width, height)
                .expect("エンコーダーが取得できること");

            assert!(
                codec_info.encoders.contains(&capabilities.encoder),
                "{codec:?} の {width}x{height} で選ばれたエンコーダーが一覧に無い: {:?}",
                capabilities.encoder
            );
        }
    }
}

/// 解像度を指定した照会の結果が解像度によって変わることを検証する
///
/// ハードウェアエンコーダーが対応しない解像度（16384x16384）では、既定の選択がソフトウェア
/// エンコーダーにフォールバックするため、選ばれるエンコーダーが変わる。
/// 「この解像度でエンコードできるか」を事前に判定できることが本照会の目的なので、
/// 解像度によって選択されるエンコーダーとハードウェア可否が変わることをここで固定する。
#[test]
fn test_query_encoding_capabilities_depends_on_resolution() {
    for codec in [VideoCodecType::H264, VideoCodecType::Hevc] {
        let hardware =
            query_encoding_capabilities(codec, 1920, 1080).expect("エンコーダーが取得できること");
        let software =
            query_encoding_capabilities(codec, 16384, 16384).expect("エンコーダーが取得できること");

        // 16384x16384 は H.264 / HEVC のどちらのハードウェアエンコーダーも対応しない解像度
        assert!(
            hardware.encoder.hardware_accelerated,
            "{codec:?} の 1920x1080 でハードウェアエンコーダーが使えない"
        );
        assert!(
            !software.encoder.hardware_accelerated,
            "{codec:?} の 16384x16384 でハードウェアエンコーダーが使えている"
        );

        assert_ne!(
            software.encoder.encoder_id, hardware.encoder.encoder_id,
            "{codec:?} の 1920x1080 と 16384x16384 で同じエンコーダーが選ばれている"
        );

        // ソフトウェアエンコーダーでもプロファイルは取得できる。
        // ハードウェア経路と同じくバリアントと中身まで固定する (空の一覧や別のバリアントが
        // 返っても is_some() だけでは検出できないため)
        match (codec, software.profiles.as_ref()) {
            (VideoCodecType::H264, Some(EncodingProfiles::H264(profiles))) => {
                assert!(
                    !profiles.is_empty(),
                    "16384x16384 の H.264 のプロファイルが空になっている"
                );
                for profile in [H264EncodingProfile::Main, H264EncodingProfile::High] {
                    assert!(
                        profiles.contains(&profile),
                        "16384x16384 の H.264 のプロファイルに {profile:?} が含まれない: {profiles:?}"
                    );
                }
            }
            (VideoCodecType::Hevc, Some(EncodingProfiles::Hevc(profiles))) => {
                assert!(
                    !profiles.is_empty(),
                    "16384x16384 の HEVC のプロファイルが空になっている"
                );
                assert!(
                    profiles.contains(&HevcEncodingProfile::Main),
                    "16384x16384 の HEVC のプロファイルに Main が含まれない: {profiles:?}"
                );
            }
            (codec, profiles) => panic!(
                "{codec:?} の 16384x16384 のプロファイルが対応するバリアントで返っていない: {profiles:?}"
            ),
        }
    }
}

/// エンコード非対応のコーデックでは、解像度を指定しても情報が取得できないことを検証する
///
/// 非対応コーデックでエンコーダーの情報が返ると、利用側が「この ID を使えばエンコードできる」と
/// 誤読する。取得できなかった値を `false` や `0` で埋めない契約をここで固定する。
#[test]
fn test_query_encoding_capabilities_unsupported_codec() {
    for codec in [VideoCodecType::Vp9, VideoCodecType::Av1] {
        let capabilities = query_encoding_capabilities(codec, 1920, 1080);

        assert_eq!(
            capabilities, None,
            "{codec:?} のエンコーダーが不明になっていない"
        );
    }
}

/// 0 や `i32` で表現できない解像度を渡しても panic せず、情報が取得できないことを検証する
///
/// Video Toolbox の解像度は `i32` で 1 以上なので、`u32` のままでは渡せない値と、
/// Video Toolbox が -12902 を返す 0 を弾く経路を通す。
#[test]
fn test_query_encoding_capabilities_invalid_resolution() {
    let cases = [
        (0, 1920),
        (1920, 0),
        (u32::MAX, 1920),
        (1920, u32::MAX),
        (u32::MAX, u32::MAX),
    ];
    for (width, height) in cases {
        let capabilities = query_encoding_capabilities(VideoCodecType::H264, width, height);

        assert_eq!(
            capabilities, None,
            "{width}x{height} のエンコーダーが不明になっていない"
        );
    }
}
