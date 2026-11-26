# 変更履歴

- UPDATE
  - 後方互換がある変更
- ADD
  - 後方互換がある追加
- CHANGE
  - 後方互換のない変更
- FIX
  - バグ修正

## develop

## 2026.2.0

**リリース日**: 2026-11-26

- [CHANGE] MSRV (rust-version) を 1.95 に上げる
  - `AtomicU64::fetch_update` が 1.99.0 で `try_update` への改名により非推奨になるため、
    1.95.0 で安定化された `AtomicU64::try_update` に置き換える
  - 開発と CI のツールチェーンも 1.95 に固定する。1.93 / 1.94 のツールチェーンではビルドできない
  - @melpon
- [CHANGE] `Encoder` と `Decoder` をコールバックベースの非同期 API に変更する
  - `EncodeHandler` トレイトと `FnEncodeHandler` ラッパー、`DecodeHandler` トレイトと
    `FnDecodeHandler` ラッパーを追加し、`Encoder::new` / `Decoder::new` にハンドラーを渡すようにする
  - `Encoder` を `Encoder<H: EncodeHandler>` に、`Decoder` を `Decoder<H: DecodeHandler>` に変更する
  - `Encoder::next_frame` を削除する。エンコード結果は `EncodeHandler::on_encoded` で受け取る
  - `Decoder::decode` の戻り値を `Result<(), Error>` に変更する。デコード結果は
    `DecodeHandler::on_decoded` で受け取る
  - `Encoder::encode` / `Encoder::encode_pixel_buffer` / `Decoder::decode` にユーザーデータを渡す
    引数を追加し、`EncodedFrame<T>` / `DecodedFrame<T>` の `user_data` として返す
  - `DecodedFrame` の `I420(I420Frame)` / `Nv12(Nv12Frame)` を
    `I420 { frame, user_data }` / `Nv12 { frame, user_data }` に変更し、`I420Frame` / `Nv12Frame`
    からライフタイムを削除する。デコード済みフレームはコールバックに所有権ごと渡される
  - `Decoder::finish` を追加する。遅延フレームを排出し、非同期コールバックの完了まで待つ
  - `Decoder::update_format` は呼び出しの先頭で `finish` を呼び、遅延フレームを排出してから
    フォーマットを更新する
  - `Decoder` の `Drop` は `finish` を呼んでからセッションを解放する
  - コールバックは Video Toolbox のコールバックスレッドから呼ばれる
  - ユーザーハンドラの panic は `extern "C"` 境界を越える前に `catch_unwind` で捕捉し、
    エラーログ（コールバック名 + panic メッセージ）を出力してセッションを継続する。
    `panic=abort` ビルドや abort する panic hook を設定している場合は捕捉できない
  - panic の捕捉は `catch_user_panic` に集約し、`invoke_callback` がコールバック名を渡す
  - @melpon
- [CHANGE] `EncoderConfig` / `H264EncoderConfig` / `HevcEncoderConfig` のプロパティ指定を `Option` にし、
  未指定なら設定せず未対応なら `Encoder::new` がエラーを返すようにする
  - `Encoder::new` はプロパティを 1 個ずつ `VTSessionSetProperty` で設定して戻り値を確認する。
    `VTSessionSetProperties` は辞書にまとめた未対応のプロパティでも `noErr` を返すため、
    エンコーダーに反映されなかった指定を検出できなかった
  - `EncoderConfig` の `prioritize_encoding_speed_over_quality` / `real_time` / `maximize_power_efficiency` /
    `allow_frame_reordering` / `allow_temporal_compression`、`H264EncoderConfig` の `profile` /
    `entropy_mode`、`HevcEncoderConfig` の `profile` / `allow_open_gop` を `Option` にし、`None` は
    「そのプロパティを設定せず Video Toolbox の既定に任せる」とする
  - 未指定時に Video Toolbox の既定に切り替わることでビットストリームが変わる項目がある。
    `allow_frame_reordering` の既定はフレーム再順序付け有効であり、`None` にすると B フレームが生成される。
    `HevcEncoderConfig::allow_open_gop` の既定も有効である
  - `max_frame_delay_count` は Apple Silicon の Apple エンコーダーが受け付けないため、指定するとエラーになる
  - `#[non_exhaustive]` ではない公開構造体のフィールドの型変更のため、構造体リテラルで構築している
    コードは `Some(...)` の追記が必要になる
  - @melpon
- [CHANGE] `EncoderConfig` に `data_rate_limits: Vec<DataRateLimit>` フィールドを追加する
  - `#[non_exhaustive]` を付けていない公開構造体へのフィールド追加のため、
    構造体リテラルで構築しているコードは `data_rate_limits: Vec::new()` の追記が必要になる
  - @voluntas
- [CHANGE] `Encoder::reconfigure` を `ReconfigureParams` ベースの動的更新専用 API に変更する
  - 旧 API は `EncoderConfig` を所有権で受け取りセッションを再作成していたが、
    `VTSessionSetProperties` 1 回で完結する動的更新型に置き換える
  - 動的に変更可能な項目は `average_bitrate` / `expected_frame_rate` の
    2 項目で、解像度・コーデック・ピクセルフォーマットの変更は `Encoder` を作り直す運用に統一する
  - セッションを再作成しないため未出力フレームの自動フラッシュも行わない。フラッシュが必要なら
    先に `Encoder::finish` を呼ぶ。全項目が `None` の場合は no-op として `Ok(())` を返す
  - `VTSessionSetProperties` が失敗した場合は `Encoder::config` の値を変更せず、セッションも生かしたままエラーを返す
  - `expected_frame_rate` 更新時は内部 PTS を切り上げで再スケールし、
    オーバーフロー時には `Error::LimitExceeded` を返す
  - @voluntas
- [CHANGE] `Encoder::new` が `average_bitrate` に 0 を指定された場合に `Error::InvalidConfig` を返すようにする
  - 従来は 0 がそのまま Video Toolbox に渡っていたが、`Encoder::reconfigure` と検証を共通化して
    構築時点で拒否する
  - @voluntas
- [CHANGE] `Encoder::new` が `max_key_frame_interval` / `max_frame_delay_count` に
  `i32::MAX` を超える値を指定された場合に `Error::InvalidConfig` を返すようにする
  - `NonZeroU32` を `i32` にキャストする際に負値へ切り詰まるのを防ぐ
  - @voluntas
- [CHANGE] `EncodedFrame::keyframe` を `picture_type` に置き換える
  - `keyframe` はキーフレームかどうかだけを表すフィールドで、`PictureType::I` かどうかと
    同じ意味になる。同じ情報を 2 通りに持つと両者がずれる余地があるため、`picture_type` に統合する
  - `EncodedFrame` から `keyframe` フィールドを削除する。キーフレームかどうかは
    `picture_type == PictureType::I` で判定する
  - `PictureType` (`I` / `P` / `B` / `Unknown`) を追加する。Video Toolbox は I フレームと
    IDR フレームを区別しないため、`PictureType::I` には IDR フレームも含まれる
  - `PictureType::B` は `allow_frame_reordering` が `Some(true)` のときと、未指定 (`None`) のときに現れる
  - @melpon
- [CHANGE] `Error::VideoToolbox` に `property` フィールドを追加する
  - プロパティを 1 個設定する `VTSessionSetProperty` が失敗した場合に、受け付けられなかった
    プロパティ名を持たせる。プロパティを指定しない失敗では `None` になる
  - `Error` は `#[non_exhaustive]` ではないため、網羅 `match` している場合はフィールドの追加が必要になる
  - @melpon
- [CHANGE] `Error` に `UnknownPixelFormat` バリアントを追加する
  - `Encoder::encode_pixel_buffer` に I420 / Nv12 のいずれでもない FourCC が渡された場合に、
    `PixelFormatMismatch` と区別して実際の FourCC を診断情報として返す
  - `Error` は `#[non_exhaustive]` ではないため、網羅 `match` している場合は分岐の追加が必要になる
  - @voluntas
- [CHANGE] `Error` 型の全 `&'static str` フィールドを `String` に変更し、エラーメッセージに動的な値を含められるようにする
  - `Option` を返していた内部関数を `Result<_, Error>` に変更し、エラー情報をコールバック経由でユーザーに伝搬する
  - 不要になった `tracing::error!()` を削除する
  - @melpon
- [CHANGE] `CodecInfo::encoding` をエンコーダー 1 件ずつの一覧 `CodecInfo::encoders` に変更し、`EncodingInfo` を 1 つのエンコーダーの情報にする
  - `EncodingInfo` の `supported` を削除する。これは対象コーデックのエンコーダーが存在するかを
    表す集計値であり、`encoders` が空かどうかで判定できる
  - `EncodingInfo` の `hardware_accelerated` / `supports_frame_reordering` / `supports_multi_pass` は、
    対象コーデックのエンコーダー全体の論理和ではなく、エンコーダー 1 件ごとの属性に変更する
  - `EncodingInfo::profiles` を削除する。利用できるプロファイルは解像度によって変わるため、
    解像度を指定して照会する API に移す
  - `EncodingProfiles` から `None` を削除する。プロファイル情報を取得できなかったことは
    `Option<EncodingProfiles>` の `None` で表す
  - `EncodingInfo` は `encoder_id` / `encoder_name` / `codec_name` / `hardware_accelerated` /
    `supports_frame_reordering` / `supports_multi_pass` / `performance_rating` / `quality_rating` /
    `has_instance_limit` を持つ
  - 解像度の上下限と数値プロパティの範囲は、Video Toolbox から信頼できる値を取得できないため追加しない
  - `CodecInfo` / `EncodingInfo` / `EncodingCapabilities` は `#[non_exhaustive]` ではないため、
    構造体リテラルで構築しているコードはフィールドの追加・変更が必要になる
  - @melpon
- [CHANGE] `DecodingInfo::supported` を削除し、`hardware_accelerated` に一本化する
  - `supported` は `hardware_accelerated` と同じ `VTIsHardwareDecodeSupported` の結果であり、
    常に同じ値になっていた。名前から「デコードできるか」と誤読されるため削除する
  - ソフトウェアデコードを含めたデコード可否は Video Toolbox から事前に取得できない。
    `VTRegisterSupplementalVideoDecoderIfAvailable` でデコーダーを登録すると結果が変わりうる
  - @melpon
- [CHANGE] `Decoder` の入力検証とエラー種別を整理する
  - `DecoderCodec::H264` / `Hevc` の `nalu_len_bytes` が 1 / 2 / 4 以外、またはパラメータセットが
    空の場合は、`Decoder::new` / `Decoder::update_format` が `Error::InvalidConfig` を返す
    (空スライスの `as_ptr()` が dangling pointer になるため)
  - VP9 / AV1 の `update_format` の Video Toolbox の失敗を、`Decoder::new` と同じく
    `Error::UnsupportedCodec` に変換する
  - @voluntas
- [ADD] `EncodedFrame` に `timestamp` と `Timestamp` 型を追加する
  - `timestamp` は `EncodeHandler::on_encoded` に渡されるフレームの提示時刻で、
    エンコーダーが入力フレームに使ったタイムスケール (`EncoderConfig::fps_numerator`) を
    分母とする有理数 `Timestamp` として取得できる。`Encoder::reconfigure` で
    フレームレートを変更すると、変更前後でタイムスケールが変わる
  - `timestamp` は `Option<Timestamp>` とし、Video Toolbox が有効な提示時刻を通知しない場合は
    `None` になる (0 を返すと先頭フレームと区別できないため)
  - `examples/raden_to_mp4.rs` が固定値ではなく取得した提示時刻からサンプルの尺を求めるようにする
  - @melpon
- [ADD] `kVTCompressionPropertyKey_DataRateLimits` に対応する `DataRateLimit` 型を追加する
  - `AverageBitRate` 指定だけでは短期ウィンドウで大きくオーバーシュートするため、
    ウィンドウあたりの総バイト数のハード上限を併設できるようにする
  - 指定できるリミットは Video Toolbox の仕様上 0〜2 個
  - Video Toolbox はエンコード開始後のこのプロパティの変更を無視するため、
    上限はセッション作成時にのみ設定できる (`Encoder` を作り直せば変更・解除できる)
  - @voluntas
- [ADD] `Encoder::config` で現在保持している `EncoderConfig` を参照する getter を追加する
  - @voluntas
- [ADD] エンコーダー / デコーダーの統計値 API (`Counter` / `Gauge` / `EncoderStats` / `DecoderStats`) を追加する
  - `Encoder::stats()` / `Decoder::stats()` で送信数・出力数・エラー数・処理中フレーム数
    (`total_encode_count` / `total_decode_count` / `total_output_frame_count` /
    `total_error_count` / `in_flight_frames`) などの通算値と現在値を取得できるようにする
  - `in_flight_frames` で Video Toolbox が処理中のフレーム数を観測でき、利用側が
    `finish()` を挟む間隔を判断できるようにする
  - デコーダーでは `update_format()` のセッション流用と再作成の回数を
    `total_update_format_count` / `total_recreate_session_count` で確認できるようにする
  - @melpon
- [ADD] `query_encoding_capabilities()` と `EncodingCapabilities` を追加する
  - `query_encoding_capabilities(codec, width, height)` で「指定した解像度でエンコードできるか」と、
    その解像度で選ばれるエンコーダー・利用できるプロファイルを事前に判定できるようにする
  - コーデックにエンコーダーが無い場合 (VP9 / AV1)、解像度が不正な場合、照会で
    エンコーダーを特定できない場合は `None` を返す。この解像度でハードウェアエンコーダーが
    使えるかは、返る `EncodingCapabilities::encoder` の `hardware_accelerated` で判定する
  - `encoderSpecification` に NULL を渡した `VTCopySupportedPropertyDictionaryForEncoder` を使う。
    これは `VTCompressionSessionCreate` と同じ既定の選択であり、
    `kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder` を付けた照会の成否と、
    選ばれるエンコーダーの `hardware_accelerated` が一致することを確認している
  - @melpon
- [UPDATE] ログ出力のクレートを `log` から `tracing` に切り替える
  - shiguredo-rust 規約の「ログは tracing を使うこと」に合わせる
  - `log::error!` を `tracing::error!` に置換し、`Cargo.toml` の依存を `tracing = "0.1"` に差し替える
  - @voluntas
- [FIX] 圧縮セッションのプロパティ設定が失敗したときにセッションがリークする問題を修正する
  - `VTCompressionSessionCreate` 成功後のセッションを `CompressionSessionGuard` で保持し、
    プロパティ設定に失敗したエラーパスでは `Drop` がセッションを無効化してから解放する
  - 成功パスでは `into_raw` でガードの所有権を放棄し、`Encoder::drop` の解放処理との二重解放を防ぐ
  - エラーパスの解放手順が `Encoder::drop` と揃う
  - @voluntas
- [FIX] PTS オーバーフロー検査がフレーム送信後に行われる問題を修正する
  - `encode` / `encode_pixel_buffer` の `checked_add` を `VTCompressionSessionEncodeFrame` 呼び出し前に移動し、
    オーバーフロー時はフレームを送信せずに `Error::LimitExceeded` を返す
  - オーバーフロー時は `next_input_pts` を変更しないため、以後の呼び出しも同じエラーを返す
  - @voluntas
- [FIX] `Encoder::encode` の NV12 入力が UV プレーンのコピーで `Error::LimitExceeded` になり必ず失敗する問題を修正する
  - プレーンのコピー幅の検査が、1 行あたりのバイト数を `CVPixelBufferGetWidthOfPlane` の戻り値と
    比較していた。バイプラナー形式のクロマプレーンではこの関数はクロマサンプル対の数を返すため、
    1 行あたり `width` バイトある UV プレーンが常に寸法超過と判定されていた
  - プレーンの寸法検査を行数 (`CVPixelBufferGetHeightOfPlane`) のみに限定し、1 行あたりのバイト数は
    コピー先のストライド (`CVPixelBufferGetBytesPerRowOfPlane`) との比較で検証するようにして、
    `pixel_format: PixelFormat::Nv12` の `Encoder` で `FrameData::Nv12` をエンコードできるようにする
  - コピー元の 1 行あたりのバイト数 `src_width` と行の間隔 `src_stride` を別の引数に分け、
    1 行あたりのバイト数とストライドを混同しないようにする。
    あわせてコピー元のデータがコピー範囲を満たすことも検証する
  - @melpon
- [FIX] エンコーダー情報の `supports_frame_reordering` が常に `false` になる問題を修正する
  - `kVTVideoEncoderList_SupportsFrameReordering` は、キーが無い場合に true と見なす仕様だが、
    キーの有無を区別せずに `false` として扱っていた
  - `CFBoolean` を取り出す処理を `Option<bool>` を返すようにし、キーごとに異なる既定値を
    呼び出し側で適用するように変更する
  - @melpon

### misc

- [UPDATE] `Encoder::reconfigure` の検証順序を `Encoder::new` と揃える
  - フレームレートの検証を bitrate の検証より先に実行し、両方を同時に不正にした場合に
    構築と再設定のどちらでもフレームレートのエラーが先に返るようにする
  - @melpon
- [UPDATE] CI / Makefile / prek.toml のコマンドフラグを統一する
  - clippy を `cargo clippy --workspace --all-targets -- -D warnings` に統一し、テストコードの
    警告を全経路で検出できるようにする
  - test を `cargo test --workspace -- --test-threads=1` に統一する
    （ハードウェアリソース競合回避のため直列実行）
  - fmt を `cargo fmt --all -- --check` に統一し、書き換えなしの検出のみにする
  - @voluntas
- [UPDATE] テスト関数に doc コメントを追加する
  - `tests/test_encoder.rs` / `tests/test_decoder.rs` / `tests/test_error.rs` の全 `#[test]`
    関数に「何を検証するか・なぜその値を使うか」を日本語の doc コメントで追記する
  - `h264_decoder` / `h265_decoder` のハードコードされたビットストリームの構成・前提条件も記す
  - @voluntas
- [UPDATE] テストのログメッセージを日本語に統一する
  - `tests/test_encoder.rs` / `tests/test_decoder.rs` / `src/encoder.rs` のテストコードの
    カスタムメッセージ（`expect` / `expect_err` / `panic!` / `unreachable!` /
    カスタムメッセージ付き `assert!` / `assert_eq!`）を日本語に統一する
  - ライブラリのログ検証文字列と `Error` の Display 出力検証は英語のまま
  - @voluntas
- [UPDATE] PBT (proptest) 基盤を導入する
  - `pbt/` ワークスペースメンバーを追加し、`make pbt` / `make pbt-with-cover` を実行可能にする
  - `Encoder::new` / `Encoder::reconfigure` の拒否域を `pbt/tests/prop_encoder.rs` で検証する
  - @voluntas
- [UPDATE] `cf_dictionary` の返り値を `CfPtr` ガード付きに統一する
  - `cf_array` / `cf_number_*` と同じく drop 時に `CFRelease` されるガードを返すようにし、呼び出し側での手動ラップを不要にする
  - @voluntas
- [UPDATE] `src/encoder.rs` をディレクトリモジュール `src/encoder/` に分割する
  - `config` / `handler` / `frame` / `validation` / `session` / `pixel_buffer` / `callback` /
    `stats` の 8 サブモジュールに責務を分離する
  - 公開 API のパスは変更しない
  - @voluntas
- [UPDATE] `encoder_rejects_*` と `reconfigure_is_noop_when_all_none` のテストアサーションを強化する
  - `encoder_rejects_*` 系が `Error::InvalidConfig` の `reason` 文字列まで検証するようにする
  - `reconfigure_is_noop_when_all_none` が no-op 後に `encode` が成功することも検証するようにする
  - @voluntas
- [UPDATE] `Encoder::reconfigure` の片肺更新 (bitrate のみ / fps のみ) の単体テストを追加する
  - 片肺更新時に「対側が不変」「更新側が反映」と fps 更新時の `fps_denominator = 1` 正規化を検証する
  - @voluntas
- [UPDATE] `Encoder::config` getter の単体テストを追加する
  - `Encoder::new` 直後の `config()` が入力した全フィールドを変更なしで返すことを検証する
  - @voluntas
- [UPDATE] `Encoder::reconfigure` のプロパティ構築をヘルパー関数 `push_bitrate_property` / `push_expected_frame_rate_property` に切り出す
  - `average_bitrate` / `expected_frame_rate` の CFNumber 構築とプロパティ辞書への push を
    モジュールレベルフリー関数にまとめ、`unsafe` はヘルパー内部のキー参照に閉じ込める
  - 挙動は変わらない
  - @voluntas
- [UPDATE] `fps_numerator` と `expected_frame_rate` の検証ロジックを共通関数 `validate_positive_i32_field` に統合する
  - ゼロ拒否と `i32::MAX` 上限拒否を `field` / `reason_overflow` を引数に取る 1 本の関数に集約する
  - エラーメッセージは従来と同一で、挙動は変わらない
  - @voluntas
- [UPDATE] `Encoder::reconfigure` 関連の rustdoc を整理して説明の重複を解消する
  - 「動的に更新できる項目 / できない項目」の本体説明を `Encoder::reconfigure` に集約し、
    `ReconfigureParams` / `Encoder::config` からは参照で辿れるようにする
  - @voluntas
- [UPDATE] `I420Frame` / `Nv12Frame` に重複していたピクセルバッファ操作を `PixelBuffer` に集約する
  - `plane_slice` / `y_plane` / `y_stride` / `width` / `height` / `Drop` を共通の内部構造体に移し、
    両フレーム型は固有のプレーンアクセサだけを持つ
  - retain と `CVPixelBufferLockBaseAddress` は `PixelBuffer::new` に集約する
  - 公開 API の変更はない（`#[derive(Debug)]` の出力形式のみ変化する）
  - @melpon
- [UPDATE] `Encoder::encode` と `Encoder::encode_pixel_buffer` に重複していたフレーム送信処理を
  `submit_pixel_buffer` に集約する
  - `frame_properties` の構築・`VTCompressionSessionEncodeFrame` の呼び出し・
    status エラー時の `user_data` の Box 回収・`next_input_pts` の更新を 1 箇所にまとめる
  - 両メソッドはピクセルバッファの取得と入力検証だけを担当する
  - 公開 API の変更はない
  - @melpon
- [UPDATE] エンコード中の `reconfigure` による更新が出力レートに反映されることを検証するテストを追加する
  - `average_bitrate` の変更 (200 kbps ↔ 8 Mbps) と `expected_frame_rate` の変更 (30 fps → 60 fps) が
    出力バイト数に現れることを確認する
  - エンコード開始後の `data_rate_limits` 変更が反映されないことを FFI を直接呼んで検出する
    (構築時に設定した上限が効くことを陽性対照として同時に確認する)
  - @melpon
- [UPDATE] 未カバーだった公開 API のテストを追加する
  - `Decoder::update_format` の description 差し替えパスとセッション再作成パス、
    および空のパラメータセット・不正な `nalu_len_bytes` の拒否を検証する
  - `Encoder::encode_pixel_buffer` の正常系と `UnknownPixelFormat` / `PixelFormatMismatch` を検証する
  - `PixelFormat::Nv12` でのデコードと `Nv12Frame` の全メソッド、
    `I420Frame` の U / V プレーン関連メソッドを PSNR で検証する
  - `Error` の Display の未テスト 5 バリアントと `std::error::Error` の実装を検証する
  - `FrameData::Nv12` のエンコード（UV プレーンの内容をデコード結果と比較して検証する）、
    キーフレームのパラメータセット、
    `nalu_len_bytes` 1 / 2 の受理、全プロファイル・エントロピー符号化モードでのセッション構築、
    ユーザー定義の `EncodeHandler` / `DecodeHandler`、`supported_codecs` のプロファイル照会も検証する
  - @melpon
- [UPDATE] 未指定のプロパティが設定されないことと、未対応のプロパティを指定したときのエラーを検証するテストを追加する
  - `max_frame_delay_count` を指定すると Apple Silicon の Apple エンコーダーが受け付けず、
    `Error::VideoToolbox` の `property` から原因のプロパティ名が分かることを確認する
  - `allow_frame_reordering` を未指定にすると、Video Toolbox の既定で B フレームが生成されることを確認する
  - @melpon
- [UPDATE] GitHub Actions の外部アクション参照をコミット SHA に固定する
  - @voluntas
- [UPDATE] `prek.toml` を shiguredo-rust 規約に合わせて整備する
  - prek 組み込みフックと tombi の lint / format を追加する
  - `cargo test` を pre-commit から pre-push に移し、コミットを遅くしないようにする
  - @voluntas
- [UPDATE] `bindgen` に `-fno-builtin` を渡し、Clang の builtin 差し替えで `size_t` が `c_ulong` として生成されるのを防ぐ
  - Rust 1.98 以降の lint でビルドが失敗するのを避けるため
  - @voluntas
- [FIX] docs.rs 向けのダミーバインディングを実際のバインディングと同じ形に揃え、ドキュメント生成が失敗する問題を修正する
  - 空の構造体で代用していた型を、実際のバインディングと同じポインタ型・型エイリアス・列挙値にする
  - `DOCS_RS` の変更で build.rs が再実行されるように `cargo::rerun-if-env-changed=DOCS_RS` を追加する
  - @melpon

## 2026.1.1

**リリース日**: 2026-04-01

- [UPDATE] `Encoder` の設定検証・プレーンコピー・パラメータ抽出・ドキュメントを堅牢化する
  - `fps_numerator` が `i32::MAX` を超える場合は `InvalidConfig` とする（`CMTimeMake` の timescale 用）
  - `copy_plane` で CVPixelBuffer のプレーン寸法・格納バイト数とコピー範囲を照合する
  - パラメータセット抽出で異常に大きい長さを拒否する（ISO/IEC 14496-15 の `unsigned int(16)` に合わせ 65535 バイト上限と根拠コメント）
  - `Encoder` と `next_frame` のドキュメントを補う（無制限 mpsc・出力 PTS 列のギャップ）
  - 同一 PTS のエンコード出力が上書きされたときにログする
  - @voluntas


## 2026.1.0

**リリース日**: 2026-04-01

- [UPDATE] VP9 / AV1 デコーダーテストで `supported_codecs()` による事前チェックを行い非対応環境ではテストをスキップする
  - @voluntas
- [ADD] `Error::CfObjectCreationFailed` バリアントを追加し、CF の辞書・数値生成が NULL を返した場合を `LimitExceeded` と区別する
  - @voluntas
- [ADD] `Error::InvalidConfig` バリアントを追加する
  - @voluntas
- [ADD] コーデック情報取得 API `supported_codecs()` を追加する
  - `VideoCodecType`, `CodecInfo`, `DecodingInfo`, `EncodingInfo` 型を追加する
  - デコード判定に `VTIsHardwareDecodeSupported` を使用する
  - エンコード判定に `VTCopyVideoEncoderList` を使用する
  - @voluntas
- [ADD] `Error::UnsupportedCodec` バリアントを追加する
  - VP9 / AV1 デコーダーが環境で利用できない場合に明確なエラーを返す
  - @voluntas
- [ADD] `Error::InsufficientFrameData` バリアントを追加する
  - @voluntas
- [ADD] `Encoder::reconfigure()` メソッドを追加する
  - @voluntas
- [ADD] `Decoder::update_format()` メソッドを追加する
  - @voluntas
- [ADD] `DecoderConfig` 構造体を追加する
  - @voluntas
- [ADD] `FrameData` enum を追加する
  - @voluntas
- [ADD] `PixelFormat` enum を追加する
  - @voluntas
- [ADD] `EncodeOptions` struct を追加し、`force_key_frame` でキーフレーム生成を強制可能にする
  - @voluntas
- [CHANGE] `Encoder::next_frame` の戻り値を `Result<Option<EncodedFrame>, Error>` にし、出力 PTS の `checked_add` 失敗時は `Error::LimitExceeded` を返す
  - `checked_add` の可否は `output_frames` から取り出す前に検証し、オーバーフロー時にエンコード済みフレームを失わない
  - `try_recv()` で受信がなくても、バッファ済みの `output_frames` から `next_output_pts` 一致分を返す
  - @voluntas
- [CHANGE] `Error::LimitExceeded` バリアントを追加する
  - CF オブジェクト生成失敗・PTS 加算オーバーフロー・プレーンコピー算術オーバーフロー等で返す
  - @voluntas
- [CHANGE] `EncoderConfig` に `pixel_format` フィールドを追加する
  - @voluntas
- [CHANGE] `Error` を enum に変更し `PixelFormatMismatch` バリアントを追加する
  - @voluntas
- [CHANGE] `DecodedFrame` を `I420Frame` / `Nv12Frame` の enum に変更する
  - @voluntas
- [CHANGE] `Decoder::new()` の引数を `DecoderConfig` に変更する
  - @voluntas
- [CHANGE] `Encoder::encode()` の引数を `FrameData` enum に変更する
  - @voluntas
- [CHANGE] `Encoder::encode()` に `options: &EncodeOptions` 引数を追加する
  - @voluntas
- [CHANGE] `Decoder::new_h264()` / `new_h265()` / `new_vp9()` / `new_av1()` を廃止し `Decoder::new(codec: DecoderCodec)` に統合する
  - @voluntas
- [CHANGE] `DecoderCodec` enum を追加し、コーデック種別と初期化パラメータを一体化する
  - @voluntas
- [CHANGE] `Decoder` の `nalu_len_bytes` / `width` / `height` を `usize` から `u32` に変更する
  - @voluntas
- [CHANGE] `ProfileLevel` enum を廃止し、コーデック固有のプロファイル enum に置き換える
  - `H264Profile` (Baseline / Main / High) と `HevcProfile` (Main / Main10) を追加する
  - @voluntas
- [CHANGE] `Encoder::new_h264()` / `Encoder::new_h265()` を廃止し `Encoder::new()` に統合する
  - `EncoderConfig` に `codec: CodecConfig` フィールドを追加し、コーデック種別とコーデック固有設定を一体化する
  - @voluntas
- [CHANGE] `EncoderConfig` から `h264_entropy_mode` と `allow_open_gop` フィールドを削除する
  - コーデック固有設定構造体 (`H264EncoderConfig` / `HevcEncoderConfig`) に移動する
  - @voluntas
- [CHANGE] `EncoderConfig` の `Default` 実装を削除する
  - 全フィールド明示を必須にする
  - @voluntas
- [CHANGE] `EncoderConfig` の整数型を `usize` から `u32` に変更する
  - `width` / `height` / `fps_numerator` / `fps_denominator` を `u32` に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` の `target_bitrate` を `usize` から `Option<u64>` に変更する
  - 未指定時はバックエンド依存とする
  - @voluntas
- [CHANGE] `EncoderConfig` の `max_key_frame_interval` / `max_frame_delay_count` を `NonZeroUsize` から `NonZeroU32` に変更する
  - @voluntas
- [CHANGE] `Encoder::new()` の引数を `&EncoderConfig` からムーブ (`EncoderConfig`) に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` のフィールド名を Video Toolbox のプロパティ名に合わせてリネームする
  - `target_bitrate` を `average_bitrate` に変更する
  - `prioritize_speed_over_quality` を `prioritize_encoding_speed_over_quality` に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` から未使用の `use_parallelization` フィールドを削除する
  - @voluntas
- [FIX] `codec_info` で Core Foundation の参照を `CFGetTypeID` なしで辞書・数値・配列・文字列として扱っていた箇所を型検証する
  - @voluntas
- [FIX] `Encoder::validate_config` で `width` / `height` が 0 の場合を拒否し、`validate_frame_data` の解像度乗算に `checked_mul` を用いる
  - @voluntas
- [FIX] `copy_plane` でプレーン基底アドレスが NULL の場合はエラーにし、`bytes_per_row` がコピー幅より小さい場合はエラーにする（行バッファ外書き込みの防止）
  - @voluntas
- [FIX] `process_encoded_output` でキーフレーム時に `CMSampleBufferGetFormatDescription` が NULL の場合は打ち切る
  - @voluntas
- [FIX] `is_keyframe` で添付が `CFDictionary` であることを `CFGetTypeID` で確認する
  - @voluntas
- [FIX] パラメータ抽出で `CMVideoFormatDescriptionRef` が NULL の場合は打ち切る
  - @voluntas
- [FIX] `Decoder::decode` と `I420Frame` / `Nv12Frame` のドキュメントを補足する
  - @voluntas
- [FIX] VP9 ラウンドトリップテストで libvpx の失敗時はテストを打ち切る
  - @voluntas
- [FIX] `Decoder::decode` で圧縮データを `Vec` にコピーしてから `CMBlockBufferCreateWithMemoryBlock` に渡す
  - @voluntas
- [FIX] `cf_dictionary` / `cf_number_*` の戻り NULL を `LimitExceeded` として扱う
  - @voluntas
- [FIX] `Encoder` の入力 / 出力 PTS を `checked_add` で更新し、オーバーフロー時はエラーまたはログする
  - @voluntas
- [FIX] `copy_plane` で `checked_mul` によりコピー長・行オフセットのオーバーフローを検出する
  - @voluntas
- [FIX] `I420Frame` / `Nv12Frame` のプレーン参照で NULL および算術オーバーフロー時は空スライスとする
  - @voluntas
- [FIX] `CMSampleBufferGetDataBuffer` が NULL のときはエンコード出力処理を行わない
  - @voluntas
- [FIX] `CMBlockBufferGetDataPointer` の結果を `vec_u8_from_raw_parts_safe` 経由でコピーする
  - @voluntas
- [FIX] `is_keyframe` で `CFArray` の要素数を確認する
  - @voluntas
- [FIX] `Encoder::validate_config` で `fps_numerator` が 0 の場合を拒否する
  - @voluntas
- [FIX] H.264 / H.265 のパラメータセット抽出でポインタとサイズの組み合わせを検証する
  - @voluntas
- [FIX] `Encoder::encode()` で必要サイズより長い入力スライスを渡した場合にヒープオーバーランが発生する問題を修正する
  - `copy_plane()` のコピーサイズを `src.len()` から `src_width * src_height` に変更する
  - @voluntas
- [FIX] `Encoder::encode()` で入力フレームのメモリ寿命が非同期エンコード中に保証されない問題を修正する
  - `CVPixelBufferCreateWithPlanarBytes` を `CVPixelBufferCreate` + データコピーに変更する
  - @voluntas
- [FIX] `fps_denominator` が 0 の場合にゼロ除算パニックが発生する問題を修正する
  - `Encoder::new()` と `Encoder::reconfigure()` で設定値を検証する
  - @voluntas
- [FIX] エンコーダーとデコーダーのコールバックで NULL 出力を考慮していない問題を修正する
  - @voluntas
- [FIX] H.264 プロファイルレベルを 3.1 固定から AutoLevel に変更する
  - @voluntas
- [FIX] `average_bitrate` の CFNumber 型を i32 から i64 に変更し高ビットレートに対応する
  - @voluntas
- [FIX] `kVTCompressionPropertyKey_PixelTransferProperties` への誤った設定を削除する
  - @voluntas
- [FIX] `Encoder::encode()` で `copy_plane` が失敗したときに `CVPixelBufferUnlockBaseAddress` が呼ばれずロックしたまま `CFRelease` され得る問題を修正する
  - ロック解除を `Drop` ガードで保証する
  - @voluntas
- [FIX] エンコードコールバックで `CMBlockBufferGetDataPointer` の戻り長だけを出力長に使うと非連続 `CMBlockBuffer` で圧縮データが途中までしか取れない問題を修正する
  - `CMBlockBufferCopyDataBytes` で `CMBlockBufferGetDataLength` 分をコピーする
  - @voluntas
- [UPDATE] README にテスト前提を記載し、`DecodingInfo`・`supported_codecs`・`DecodedFrame` / `I420Frame` / `Nv12Frame` の rustdoc を補う
  - @voluntas
- [FIX] `Encoder::encode` で `VTCompressionSessionEncodeFrame` 呼び出し前に `CVPixelBuffer` をアンロックする
  - @voluntas
- [FIX] エンコード出力で `CMBlockBufferGetDataLength` が防御的上限を超える場合はログして当該フレームを破棄する
  - @voluntas
- [FIX] `Encoder::validate_config` で `width` / `height` が `i32::MAX` を超える場合を拒否し、`DecoderCodec::Vp9` / `Av1` の `CMVideoFormatDescriptionCreate` 呼び出し前に同じ寸法範囲を検証する
  - `Decoder::wrap_unsupported_codec_error` は `VideoToolbox` エラーのみ `UnsupportedCodec` に変換し、`InvalidConfig` はそのまま返す
  - @voluntas
- [FIX] `EncoderConfig` の `average_bitrate` が `i64::MAX` を超える `u64` のときに `CFNumber` へ負の値が渡るのを防ぐため、`InvalidConfig` で拒否する
  - @voluntas

### misc

- CI の `release/**` ブランチ push でも GitHub Actions を実行する
  - @voluntas
- README に VP9 / AV1 デコード初期化時の `UnsupportedCodec` と `InvalidConfig` の違いを追記する
  - @voluntas
- VP9 デコーダーテストを追加する
  - shiguredo_libvpx でカラーバーをエンコードし Video Toolbox でデコードして PSNR を検証する
  - @voluntas

## 2025.1.0

**リリース日**: 2025-09-26
