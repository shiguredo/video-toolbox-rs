//! `Encoder` モジュールの検証系ロジックの拒否域を PBT で検証する
//!
//! 実ハードウェア (Video Toolbox) を叩く FFI クレートのため、PBT は公開 API 経由で
//! 到達可能な純ロジック (拒否域) に限定する。受理域は実 FFI セッション生成を伴うため
//! 対象外とし、境界値の受理・拒否は単体テストがカバーする。

use std::{cell::RefCell, num::NonZeroU32, time::Duration};

use proptest::prelude::*;
use shiguredo_video_toolbox::{
    CodecConfig, DataRateLimit, Encoder, EncoderConfig, Error, FnEncodeHandler, H264EncoderConfig,
    H264EntropyMode, H264Profile, PixelFormat, ReconfigureParams,
};

/// 拒否域テスト用の no-op ハンドラー
///
/// 拒否される入力は検証で弾かれるためエンコードコールバックは呼ばれず、no-op でよい。
fn noop_encode_handler() -> FnEncodeHandler<()> {
    FnEncodeHandler::new(|_: Result<_, Error>| {})
}

/// `validate_config` を通過する EncoderConfig を生成する
///
/// 拒否域テストの「不正フィールドを 1 個だけ固定し、残りは有効範囲から生成する」戦略の
/// 基底として使う。任意値ベースの naive な生成にすると有効な EncoderConfig が生成されて
/// 実 FFI セッションが作られてしまうため、各フィールドは検証を通過する範囲に絞る。
/// なお、この戦略が検証を通過すること自体はテストしていない。将来 `validate_config` に
/// 拒否パスが追加された場合は本戦略の範囲も見直すこと。
fn valid_config_strategy() -> impl Strategy<Value = EncoderConfig> {
    (
        1u32..=i32::MAX as u32,                                           // width
        1u32..=i32::MAX as u32,                                           // height
        1u32..=i32::MAX as u32,                                           // fps_numerator
        1u32..=u32::MAX, // fps_denominator (ゼロ拒否のみ)
        prop_oneof![Just(None), (1u64..=i64::MAX as u64).prop_map(Some)], // average_bitrate
        prop_oneof![
            Just(None),
            (1u32..=i32::MAX as u32).prop_map(|n| Some(NonZeroU32::new(n).expect("n >= 1")))
        ], // max_key_frame_interval
        prop_oneof![
            Just(None),
            (1u32..=i32::MAX as u32).prop_map(|n| Some(NonZeroU32::new(n).expect("n >= 1")))
        ], // max_frame_delay_count
    )
        .prop_map(
            |(
                width,
                height,
                fps_numerator,
                fps_denominator,
                average_bitrate,
                max_key_frame_interval,
                max_frame_delay_count,
            )| EncoderConfig {
                width,
                height,
                codec: CodecConfig::H264(H264EncoderConfig {
                    profile: H264Profile::Main,
                    entropy_mode: H264EntropyMode::Cabac,
                }),
                pixel_format: PixelFormat::I420,
                average_bitrate,
                fps_numerator,
                fps_denominator,
                prioritize_encoding_speed_over_quality: false,
                real_time: false,
                maximize_power_efficiency: false,
                allow_frame_reordering: false,
                allow_temporal_compression: true,
                max_key_frame_interval,
                max_key_frame_interval_duration: None,
                max_frame_delay_count,
                data_rate_limits: None,
            },
        )
}

/// `Encoder::new` が拒否する不正な EncoderConfig を生成する
///
/// 「不正フィールドを 1 個だけ固定し、残りのフィールドは有効範囲から生成する」戦略。
/// `validate_config` の全拒否パス (15 個) を 1 ケースずつ列挙する。proptest の
/// ランダムサンプリングにより全ケースの実行は保証されないが、欠落確率は無視できる程度
/// ((14/15)^256 相当) である。
/// 拒否される入力は `validate_config` で弾かれるため、`create_compression_session` には
/// 到達せず実機セッションなしで PBT を実行できる。
fn invalid_config_strategy() -> impl Strategy<Value = EncoderConfig> {
    prop_oneof![
        // width / height のゼロと i32::MAX 超え
        valid_config_strategy().prop_map(|mut c| {
            c.width = 0;
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.height = 0;
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.width = i32::MAX as u32 + 1;
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.height = i32::MAX as u32 + 1;
            c
        }),
        // fps_numerator のゼロと i32::MAX 超え、fps_denominator のゼロ
        valid_config_strategy().prop_map(|mut c| {
            c.fps_numerator = 0;
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.fps_numerator = i32::MAX as u32 + 1;
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.fps_denominator = 0;
            c
        }),
        // average_bitrate のゼロと i64::MAX 超え
        valid_config_strategy().prop_map(|mut c| {
            c.average_bitrate = Some(0);
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.average_bitrate = Some(i64::MAX as u64 + 1);
            c
        }),
        // data_rate_limits の個数超過と内部検証の各拒否
        valid_config_strategy().prop_map(|mut c| {
            c.data_rate_limits = Some(vec![
                DataRateLimit {
                    bytes: 1,
                    window: Duration::from_secs(1),
                };
                3
            ]);
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.data_rate_limits = Some(vec![DataRateLimit {
                bytes: 0,
                window: Duration::from_secs(1),
            }]);
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.data_rate_limits = Some(vec![DataRateLimit {
                bytes: i64::MAX as u64 + 1,
                window: Duration::from_secs(1),
            }]);
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.data_rate_limits = Some(vec![DataRateLimit {
                bytes: 1,
                window: Duration::ZERO,
            }]);
            c
        }),
        // max_key_frame_interval / max_frame_delay_count の i32::MAX 超え
        valid_config_strategy().prop_map(|mut c| {
            c.max_key_frame_interval = NonZeroU32::new(i32::MAX as u32 + 1);
            c
        }),
        valid_config_strategy().prop_map(|mut c| {
            c.max_frame_delay_count = NonZeroU32::new(i32::MAX as u32 + 1);
            c
        }),
    ]
}

proptest! {
    /// 不正な EncoderConfig は必ず Error::InvalidConfig で拒否されること
    #[test]
    fn encoder_new_rejects_invalid_config(config in invalid_config_strategy()) {
        let result = Encoder::new(config, noop_encode_handler());
        let err = match result {
            Ok(_) => panic!("不正な config は拒否されること"),
            Err(e) => e,
        };
        let is_invalid_config = matches!(err, Error::InvalidConfig { .. });
        prop_assert!(is_invalid_config, "InvalidConfig を期待したが、実際は: {err}");
    }
}

/// reconfigure の拒否域テストで使い回す有効な Encoder を構築する
///
/// 実 FFI セッションを 1 つだけ生成し、全ケースで使い回す。
/// 拒否される入力は `validate_reconfigure_params` で弾かれるため、
/// `VTSessionSetProperties` には到達せずセッションの状態は変わらない。
fn valid_encoder() -> Encoder<FnEncodeHandler<()>> {
    let config = EncoderConfig {
        width: 640,
        height: 480,
        codec: CodecConfig::H264(H264EncoderConfig {
            profile: H264Profile::Main,
            entropy_mode: H264EntropyMode::Cabac,
        }),
        pixel_format: PixelFormat::I420,
        average_bitrate: Some(100_000),
        fps_numerator: 30,
        fps_denominator: 1,
        prioritize_encoding_speed_over_quality: false,
        real_time: false,
        maximize_power_efficiency: false,
        allow_frame_reordering: false,
        allow_temporal_compression: true,
        max_key_frame_interval: None,
        max_key_frame_interval_duration: None,
        max_frame_delay_count: None,
        data_rate_limits: None,
    };
    Encoder::new(config, noop_encode_handler()).expect("valid config must be accepted")
}

/// `Encoder::reconfigure` が拒否する不正な ReconfigureParams を生成する
///
/// `validate_reconfigure_params` の全拒否パス (8 個) を 1 ケースずつ列挙する。proptest の
/// ランダムサンプリングにより全ケースの実行は保証されないが、欠落確率は無視できる程度
/// ((7/8)^256 相当) である。
fn invalid_params_strategy() -> impl Strategy<Value = ReconfigureParams> {
    prop_oneof![
        // average_bitrate のゼロと i64::MAX 超え
        Just(ReconfigureParams {
            average_bitrate: Some(0),
            ..Default::default()
        }),
        Just(ReconfigureParams {
            average_bitrate: Some(i64::MAX as u64 + 1),
            ..Default::default()
        }),
        // expected_frame_rate のゼロと i32::MAX 超え
        Just(ReconfigureParams {
            expected_frame_rate: Some(0),
            ..Default::default()
        }),
        Just(ReconfigureParams {
            expected_frame_rate: Some(i32::MAX as u32 + 1),
            ..Default::default()
        }),
        // data_rate_limits の個数超過と内部検証の各拒否
        Just(ReconfigureParams {
            data_rate_limits: Some(vec![
                DataRateLimit {
                    bytes: 1,
                    window: Duration::from_secs(1),
                };
                3
            ]),
            ..Default::default()
        }),
        Just(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 0,
                window: Duration::from_secs(1),
            }]),
            ..Default::default()
        }),
        Just(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: i64::MAX as u64 + 1,
                window: Duration::from_secs(1),
            }]),
            ..Default::default()
        }),
        Just(ReconfigureParams {
            data_rate_limits: Some(vec![DataRateLimit {
                bytes: 1,
                window: Duration::ZERO,
            }]),
            ..Default::default()
        }),
    ]
}

/// 不正な ReconfigureParams は必ず Error::InvalidConfig で拒否され、config が不変であること
#[test]
fn encoder_reconfigure_rejects_invalid_params() {
    // proptest のクロージャは Fn を要求するため、使い回す Encoder を RefCell で包む。
    // proptest はデフォルトでシングルスレッド実行のため borrow_mut は競合しない。
    let encoder = RefCell::new(valid_encoder());
    proptest!(|(params in invalid_params_strategy())| {
        let mut guard = encoder.borrow_mut();
        let before_bitrate = guard.config().average_bitrate;
        let before_fps_num = guard.config().fps_numerator;
        let before_fps_den = guard.config().fps_denominator;
        let before_limits = guard.config().data_rate_limits.clone();

        let result = guard.reconfigure(params);
        let err = match result {
            Ok(_) => panic!("不正な params は拒否されること"),
            Err(e) => e,
        };
        let is_invalid_config = matches!(err, Error::InvalidConfig { .. });
        prop_assert!(is_invalid_config, "InvalidConfig を期待したが、実際は: {err}");
        // 拒否時は config が変更されないこと
        prop_assert_eq!(guard.config().average_bitrate, before_bitrate);
        prop_assert_eq!(guard.config().fps_numerator, before_fps_num);
        prop_assert_eq!(guard.config().fps_denominator, before_fps_den);
        prop_assert_eq!(guard.config().data_rate_limits.clone(), before_limits);
    });
}
