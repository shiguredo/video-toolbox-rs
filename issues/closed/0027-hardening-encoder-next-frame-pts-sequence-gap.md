# `Encoder::next_frame` が期待する PTS 列と実出力にギャップがあるとフレームが取り出せず詰まる

Created: 2026-04-01
Model: Composer 2 Fast

## 分類

ファイル名は **`hardening`**。観測根拠が限定的なため **`bug` と名乗らない**（レビュー指摘）。

## なぜこの対応が必要か

`next_frame` は **`self.next_output_pts` と一致するキー**が `output_frames` に存在するときだけ取出し、成功後に **`next_output_pts` を `fps_denominator` 分だけ加算**する。**届いたフレームの PTS がこの期待列から外れる**（欠番・飛び・順不同）と、**`contains_key(&next_output_pts)` が常に false** になり続け、**`next_output_pts` が進まない**。一方で別 PTS のフレームは `HashMap` に溜まるため、**受信ループが止まらなければメモリに滞留**しうる。`allow_frame_reordering: false` 前提のドキュメントはあるが、**実バックエンドの出力が前提とズレる**と問題になりうる。

## 現状

- **場所**: `src/lib.rs` の `Encoder::next_frame`（`contains_key` / `remove` / `next_output_pts` 更新）

## 問題

- PTS ギャップ時の検出・回復・ログがない。

## 望ましい対応の方向（案）

- タイムアウト・最小 PTS の取出し・ログ・または **仕様としてサポートしない組み合わせを明示**する。
- **入力 PTS**（`next_input_pts` の進み）と **出力 `pts.value`** が **同じ量子化**であることの前提を文書化（ズレると本現象が起きやすい）。

## 解決の完了条件（厳格）

- 「`allow_frame_reordering: false` なら安全」だけでは **不十分**。**出力 PTS の列が `fps_denominator` 刻みで連続するとは限らない**ことを認めたうえで、**サポート範囲**または **検出時の挙動**が書かれていること。

## 厳格レビュー（第 2 パス）

1. **ソース照合**: `contains_key` → `remove` → `checked_add` の順を確認。
2. **不足していた観点**: **入力と出力の PTS 対応**（前回は一文に含めなかった）。
3. **0026 との関係**: 欠番でも **別 PTS のフレームは HashMap に溜まる**ため、**メモリ**面では 0030 と相関。
4. **誤解しやすい点**: `try_recv` が空でも **`output_frames` に残存**しうるため、「受信しなくても進む」は **条件付き**。
5. **前回からの差分**: 完了条件で **一文のドキュメント依存を却下**。

## 前任レビューで甘かった点

- `allow_frame_reordering` の説明に **過度に依存**し、**PTS 値の実際の列**についての検証・記述が薄かった。

## 解決方法

Completed: 2026-04-01

- `next_frame` の公開ドキュメントに、`next_output_pts` の期待列と実出力 PTS がずれた場合の挙動（`None` の繰り返し・メモリ滞留）および入力 `CMTimeMake` と出力 `value` のスケール前提を追記した。
