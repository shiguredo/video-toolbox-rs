# `I420Frame` / `Nv12Frame` の重複を解消する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-dedup-frame-types
- Polished: 2026-08-01

## 目的

`src/decoder.rs` の `I420Frame` と `Nv12Frame` で `plane_slice` / `y_plane` / `y_stride` / `width` / `height` / `Drop` 実装がほぼ同一（コード部が文字単位で同一）。`plane_slice` は関数本体（NULL チェック・オーバーフローチェック・`from_raw_parts`）が完全に同一。修正時に片方だけ漏れるリスクがある（2026.1.0 で両型へ同一の NULL チェック・オーバーフロー修正を適用した実績がある）。

## 現状

`I420Frame` と `Nv12Frame` がそれぞれ独立に同一ロジックを実装している。`plane_slice` の NULL チェック・オーバーフローチェック・`from_raw_parts` の呼び出しが文字単位で同一。`y_plane` / `y_stride` / `width` / `height` / `Drop` も両型で同一。

## 設計方針

共通の内部構造体 `LockedPixelBuffer` を新設し、両型で同一の実装を集約する:

- `plane_slice` / `y_plane` / `y_stride` / `width` / `height` / `Drop` を `LockedPixelBuffer` に集約する
- `I420Frame` / `Nv12Frame` は `LockedPixelBuffer` をラップし、固有のプレーンアクセサ（I420: `u_plane` / `v_plane` / `u_stride` / `v_stride`、Nv12: `uv_plane` / `uv_stride`）だけを持つ。`y_plane` / `y_stride` / `width` / `height` などの共通アクセサは `LockedPixelBuffer` への委譲として再公開し、公開 API を維持する
- `Drop` を `LockedPixelBuffer` に移す際は `I420Frame` / `Nv12Frame` の `Drop` 実装を削除する（残すと二重アンロックになる）
- stride 系（`y_stride` / `u_stride` / `v_stride` / `uv_stride`）は `CVPixelBufferGetBytesPerRowOfPlane` の呼び方が共通なので、`LockedPixelBuffer` に `plane_stride(plane_index)` を置いて一元化する

なお、`#[derive(Debug)]` の出力形式が `inner: CfPtrMut(...)` → `buffer: LockedPixelBuffer { inner: ... }` に変わる（公開 API 契約外の挙動差として認識しておく）。

### 検証方法

`cargo test --workspace -- --test-threads=1` で既存テストが通ることを確認する。なお `I420Frame` / `Nv12Frame` のプレーンアクセサの多く（Nv12 の全メソッドは issue 0071 の対象。I420 の `u_plane` / `v_plane` / `u_stride` / `v_stride` は対象外）は既存テストで未検証のため、自動検査では退行を検出できない。委譲後のプレーンインデックス（`u` / `v` / `uv`）が既存実装と 1 行ずつ一致することを手動で照合する。

## 完了条件

- `plane_slice` の実装が 1 箇所に集約されていること
- `I420Frame` / `Nv12Frame` の `Drop` 実装が削除されていること（二重アンロックの防止）
- `I420Frame` / `Nv12Frame` の公開 API の変更がないこと
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0071: `Nv12Frame` の公開メソッドのテスト追加を対象とする。公開 API は変わらず開発順序の制約はないが、同一ファイル（`src/decoder.rs` / `tests/test_decoder.rs`）を変更するためマージ時は差分衝突に注意する
- issue 0073: `encode` / `encode_pixel_buffer` の重複解消で、本 issue と同じ「重複解消」の refactor カテゴリ
