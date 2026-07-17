# `Encoder` の `output_frames`（HashMap）で同一 PTS が二度届くと先のフレームが上書きされる

Created: 2026-04-01
Model: Composer 2 Fast

## 分類

ファイル名は **`hardening`**。再現ログや「必ず起きる不具合」までは言い切れない **設計上の懸念**として追う（レビューでは `bug` より `hardening` / design が自然との指摘）。

## なぜこの対応が必要か

`next_frame` は `try_recv` で受け取った `EncodedFrame` を **`frame.pts` をキーに** `output_frames.insert` する。**同一キー**が再度来ると `HashMap` は **後勝ち**であり、**先に届いたフレームは破棄**される。キーは `CMSampleBufferGetPresentationTimeStamp(...).value`（`CMTime.value`）由来で、**バックエンドが同一時刻を二重に返す**（理論上）と発生しうる。

## 現状

- **場所**: `src/lib.rs` の `Encoder::next_frame` 内の `self.output_frames.insert(frame.pts, frame)`。
- **経路**: コールバック → `mpsc` → `try_recv` → `insert` のみ。

## 問題

- 重複検知・ログ・エラー方針がない。
- **前任レビューで甘かった点**: 「同一 PTS」と書きつつ、**`pts` のスケール（timescale）と `value` の整合**が `next_output_pts` の進め方と **同一前提か**は未検証。厳格には **キー空間の定義**を仕様に書く必要がある。

## 望ましい対応の方向（案）

- `insert` の戻り `Some(old)` で **上書きを検知**し、ログまたはエラー。
- 仕様書に **PTS キーの一意性期待**と、違反時の挙動（エラー・ログ・後勝ち明示）を書く。

## 解決の完了条件（厳格）

- **上書きが発生しうる**ことを公開ドキュメントかコメントで **否定していない**こと。
- 実装で **検知**するか、**「起こらないことをバックエンドに要求する」**かのどちらかが明示されていること。

## 厳格レビュー（第 2 パス）

1. **ソース照合**: `EncodedFrame { pts: pts.value }` と `insert` を確認。
2. **不足していた観点**: **timescale と value** の仕様上の前提。
3. **0027 との違い**: 本 issue は **同一キー上書き**、0027 は **期待キー欠番**。
4. **誤解しやすい点**: 「HashMap だから重複しない」は **誤り**（後勝ち）。
5. **前回からの差分**: 完了条件で **文書での否定禁止**を追加。

## 解決方法

Completed: 2026-04-01

- `next_frame` の `insert` で `Some` が返ったときに英語の `log::warn!` で同一 PTS 上書きを記録する。
