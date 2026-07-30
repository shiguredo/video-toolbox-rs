# `I420Frame` / `Nv12Frame` の重複を解消する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-dedup-frame-types
- Polished: {YYYY-MM-DD}

## 目的

`src/decoder.rs` の `I420Frame` と `Nv12Frame` で `plane_slice` / `y_plane` / `y_stride` / `width` / `height` / `Drop` 実装がほぼ同一。`plane_slice` は完全に同一の 12 行。修正時に片方だけ漏れるリスクがある。

## 現状

`I420Frame` と `Nv12Frame` がそれぞれ独立に同一ロジックを実装している。`plane_slice` の NULL チェック・オーバーフローチェック・`from_raw_parts` の呼び出しが文字単位で同一。

## 設計方針

共通の内部構造体（例: `LockedPixelBuffer`）に `plane_slice` / `width` / `height` / `Drop` を集約し、`I420Frame` / `Nv12Frame` はそれをラップして固有のプレーンアクセサだけ持つ形にする。

## 完了条件

`plane_slice` の実装が 1 箇所に集約されていること。公開 API の変更はないこと。
