# docs.rs 向けダミーバインディングが不完全でビルドが失敗する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-docs-rs-dummy-bindings
- Polished: {YYYY-MM-DD}

## 目的

`build.rs` の docs.rs 向けダミーバインディングが構造体 17 個の定義のみで、src/ 全体で参照している関数・定数・型が欠落している。docs.rs（Linux + `cfg(doc)`）でコンパイルが通らない可能性が高い。

## 現状

`build.rs` の `DOCS_RS` 分岐で出力するダミーバインディングは `CFDictionaryRef` / `CMTime` 等の構造体のみの定義。以下の項目が欠落している:

- 関数: `CFRelease` / `CFRetain` / `CFDictionaryCreate` / `CVPixelBufferCreate` / `VTCompressionSessionEncodeFrame` / `CMTimeMake` 等（計 50 以上）
- 定数: `kCFBooleanTrue` / `kVTCompressionPropertyKey_*` / `kCVPixelFormatType_*` / `kCMTimeInvalid` 等（計 30 以上）
- 型: `CFTypeRef` / `CFNumberType` / `VTDecompressionOutputCallbackRecord` 等

CI の `docs-rs` ジョブ（`.github/workflows/ci.yml`）が実際に通過しているか確認が必要。

## 設計方針

ダミー定義に関数シグネチャ・定数・不足型を全て追加するか、`#[cfg(target_os = "macos")]` でモジュール全体をガードし docs.rs では公開 API の型定義のみをコンパイル対象にする構成に変更する。

## 完了条件

`DOCS_RS=1 cargo doc --no-deps` が Linux 上で成功すること。
