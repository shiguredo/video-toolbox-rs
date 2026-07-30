# 未カバーの公開 API テストを追加する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/add-uncovered-api-tests
- Polished: {YYYY-MM-DD}

## 目的

公開 API のうち以下のテストが存在しない。shiguredo-rust 規約は「公開 API に対応するテストが存在するか」を求めている。

## 現状

未カバーの公開 API:

- `Decoder::update_format`（`src/decoder.rs`）: セッション再作成パスと description 差し替えパスのいずれも未検証
- `Encoder::encode_pixel_buffer`（`src/encoder.rs`）: unsafe 公開 API だがテストゼロ。`UnknownPixelFormat` / `PixelFormatMismatch` のエラーパスはここでしか到達しない
- `PixelFormat::Nv12` でのデコードパス: `Nv12Frame` の全メソッド（`y_plane` / `uv_plane` / `y_stride` / `uv_stride` / `width` / `height`）が未到達
- `Error` の Display: 8 バリアント中 5 つ（`VideoToolbox` / `PixelFormatMismatch` / `InsufficientFrameData` / `UnsupportedCodec` / `InvalidConfig`）が未テスト

## 完了条件

上記の公開 API すべてに対応するテストが存在すること。
