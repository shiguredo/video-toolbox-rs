# `CMTimeMake` の timescale に `fps_numerator as i32` を使うと負の timescale になり得る

Created: 2026-04-01
Completed: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

`CMTimeMake(self.next_input_pts, self.config.fps_numerator as i32)` の **第 2 引数は timescale**。`fps_numerator` は **`u32`** だが **`i32` に無検査キャスト**している。Rust では **`u32::MAX as i32 == -1`** のように、**`fps_numerator > i32::MAX as u32` のとき timescale が負**になり得る。`validate_config` は **`fps_numerator != 0` のみ**で、**上限は検証していない**。これは **文書の問題にとどまらない**（少なくとも **拒否または `try_from`** が論点）。

## 現状

- **場所**: `src/lib.rs` の `Encoder::encode` / `encode_pixel_buffer` の `CMTimeMake`
- **補足**: `validate_config` は `fps_numerator == 0` のみ拒否。

## 問題（リスク）

- **負の timescale** による `CMTime` の意味が **未定義に近い**（VT の挙動は実装依存）。
- **長時間稼働**など **`next_input_pts` の大きさ**と組み合わさったときの解釈も **文書化されていない**。
- **前任レビューで甘かった点**: 「`as i32` の範囲」に触れたが、**`u32 → i32` が負に落ちる**具体例と **`validate_config` に上限がない**事実を **書いていなかった**（前回レビューは **誤り**）。

## 望ましい対応の方向（案）

- **`fps_numerator` を `i32` に収まる範囲に制限**する（`validate_config` で `<= i32::MAX as u32`）、または `CMTime` 構築 API を **`i32` に依存しない**形に変更する（要調査）。
- **推奨する PTS 値域**をドキュメント化（上記の **負 timescale バグ**を直した後の話としても可）。

## 解決の完了条件（厳格）

- **負の timescale が生成されうる経路**を **残したまま「文書だけ」**は **不合格**。
- 少なくとも **`fps_numerator as i32` が負になり得る**ことを **仕様またはバグ**として扱い、**コードまたは拒否**のどちらかで潰すこと。

## 厳格レビュー（第 2 パス）

1. **ソース照合**: `CMTimeMake` と `validate_config` を再確認。**`fps_numerator` 上限なし**を確認。
2. **不足していた観点（前回の誤り）**: **`u32 as i32` の符号拡張**。
3. **0027 との差分**: 入力 PTS と出力 PTS は別。
4. **誤解しやすい点**: `checked_add` は **PTS 加算**の安全のみ。timescale は **別フィールド**。
5. **前回からの差分**: **実バグの可能性**を issue に昇格。

## 解決方法

- `validate_config` で `fps_numerator > i32::MAX as u32` を `InvalidConfig` として拒否する。単体テスト `encoder_rejects_fps_numerator_above_i32_max` を追加した。
