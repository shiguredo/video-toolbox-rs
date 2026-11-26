# docs.rs 向けダミーバインディングが不完全でビルドが失敗する

- Created: 2026-07-30
- Completed: 2026-08-01
- Branch: feature/fix-docs-rs-dummy-bindings

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

## 解決方法

本 issue は polish-issue の不要判定でスキップされた（実装しない）。`DOCS_RS=1 cargo doc --no-deps` は実測で成功しており（rustdoc は関数本体の型チェックをしないため）、CI の docs-rs ジョブも success を継続している。完了条件は既に満たされており、docs.rs 本番のビルド（`cargo rustdoc` のみ実行）も失敗しない。crates.io 未公開のため本番ビルドは未発生。ダミーバインディングの不足は `DOCS_RS=1 cargo check` でのみ発現するが、どこも実行しないため実害はない。
