# 未カバーの公開 API テストを追加する

- Created: 2026-07-30
- Completed: 2026-10-02
- Branch: feature/add-uncovered-api-tests
- Polished: 2026-08-01

## 目的

公開 API のうち以下のテストが存在しない。shiguredo-rust 規約（「`tests/`・`pbt/`・`fuzz/` のテストは公開 API に対してだけ書くこと」）の要請に沿って、未カバーの公開 API にテストを追加する。

## 現状

未カバーの公開 API:

- `Decoder::update_format`（`src/decoder.rs`）: セッション再作成パスと description 差し替えパスのいずれも未検証
- `Encoder::encode_pixel_buffer`（`src/encoder.rs`）: unsafe 公開 API だがテストゼロ。`UnknownPixelFormat` はここでしか到達しない（`PixelFormatMismatch` は `encode` 経由では検証済みだが、`encode_pixel_buffer` 経由では未検証）
- `PixelFormat::Nv12` でのデコードパス: `Nv12Frame` の全メソッド（`y_plane` / `uv_plane` / `y_stride` / `uv_stride` / `width` / `height`）が未到達
- `Error` の Display: 8 バリアント中 5 つ（`VideoToolbox` / `PixelFormatMismatch` / `InsufficientFrameData` / `UnsupportedCodec` / `InvalidConfig`）が未テスト

## 設計方針

### update_format

`VTDecompressionSessionCanAcceptFormatDescription` の戻り値は不透明で、テストから「どのパスを通ったか」を直接判定できない。description 差し替えパスは同一 SPS/PPS での再呼び出しで発火する。セッション再作成パスはコーデック変更（H.264→HEVC）で確実に発火する（同一コーデックの解像度変更は `VTDecompressionSessionCanAcceptFormatDescription` が受理し得るため、description 差し替えパスを通る可能性がある）。VP9/AV1 への変更は非対応環境で `UnsupportedCodec` になるため、既存の `vp9_decoder` テストと同様に `supported_codecs()` による事前チェックを入れる。両パスの区別は update_format 後のデコード結果（HEVC ビットストリームのデコード成功など）で観測する。

### encode_pixel_buffer

`sys` モジュールは private のため、テストから `CVPixelBufferCreate` を直接呼べない。テスト側で extern FFI の自前宣言が必要（issue 0061 の `CVPixelBufferCreate` 前準備はモジュール内テスト用のため `tests/` からは再利用できない。テスト間で共有する場合は `tests/helpers/` への配置を検討する）。`UnknownPixelFormat` は不正な FourCC、`PixelFormatMismatch` は有効な別フォーマットの CVPixelBuffer で検証する。

### Nv12 デコード

出力フォーマットはビットストリームではなく `DecoderConfig.pixel_format` で決まるため、専用の Nv12 ビットストリームは不要。既存の H.264 / H.265 ビットストリームを `pixel_format: PixelFormat::Nv12` でデコードすれば全メソッドを検証できる。

## 完了条件

- `Decoder::update_format` の両パス（セッション再作成 / description 差し替え）を検証するテストが存在すること
- `Encoder::encode_pixel_buffer` の正常系と `UnknownPixelFormat` / `PixelFormatMismatch` のエラーパスを検証するテストが存在すること
- `PixelFormat::Nv12` でのデコードパスと `Nv12Frame` の全メソッドを検証するテストが存在すること
- `Error` の Display の未テスト 5 バリアントを検証するテストが存在すること
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0048: `Encoder::config` 以外の未カバー公開 API のテストを本 issue で扱うと委譲されている
- issue 0061: `encode_pixel_buffer` のテストで `CVPixelBufferCreate` の前準備を計画している
- issue 0072: `I420Frame` / `Nv12Frame` の重複解消が同一領域に触れる（公開 API は変わらないため並行可能）
- issue 0073: `encode` / `encode_pixel_buffer` の重複解消が同一領域に触れる（公開 API は変わらないため並行可能）

## 解決方法

完了条件を満たすテストを追加する。

- `Decoder::update_format`: description 差し替えパスは、更新前のキーフレームを参照する P フレームを更新後にデコードして成功することで検証する。セッション再作成パスは H.264 から H.265 への変更で検証する（`update_format_with_same_parameter_sets_keeps_reference_frames` / `update_format_switching_to_h265_recreates_session`）
- `Encoder::encode_pixel_buffer`: 正常系と、不正な FourCC による `UnknownPixelFormat`、別フォーマットの CVPixelBuffer による `PixelFormatMismatch` を検証する
- `PixelFormat::Nv12` でのデコード: `DecoderConfig.pixel_format` に NV12 を指定して `Nv12Frame` の全メソッドを PSNR で検証する
- `Error` の Display: 未テストだった 5 バリアントと `std::error::Error` の実装を検証する
- `FrameData::Nv12` のエンコードを検証する過程で、`Encoder::encode` の UV プレーンのコピーが `CVPixelBufferGetWidthOfPlane` の戻り値（クロマサンプル対の数）を 1 行あたりのバイト数として比較し、必ず `Error::LimitExceeded` になる不具合を見つけた。同じブランチで修正し、`encode_nv12_frame_preserves_plane_content` でデコード結果と入力プレーンの内容を比較して検証する
