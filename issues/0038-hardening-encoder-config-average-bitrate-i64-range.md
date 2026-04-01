# `average_bitrate` を `u64 as i64` で渡すと大きな値が負のビットレートになる

Created: 2026-04-01  
Model: GPT-5.2

## なぜこの対応が必要か

`add_common_properties` 内で `config.average_bitrate` を `cf_number_i64(bitrate as i64)` として渡している（`src/lib.rs`）。`bitrate` の型は **`Option<u64>`** の **`u64`** である。

Rust では **`u64::MAX as i64` は `-1`** のように、**`i64::MAX` を超える `u64` を `as i64` すると負の値**になる。極端に大きなビットレート指定は、Core Foundation 経由で **負のビットレート**としてエンコーダに渡りうる。挙動は **バックエンド依存**で、エラー・無視・未定義に近い振る舞いの余地がある。

## 現状

- **場所**: `Encoder::add_common_properties` の `average_bitrate` 分岐。
- **検証**: `validate_config` は **`average_bitrate` を検証していない**。

## 望ましい対応の方向（案）

- **`bitrate <= i64::MAX as u64` を満たさない場合は `InvalidConfig` で拒否**する（または `try_into::<i64>()` で失敗時に拒否）。
- または **ドキュメントで上限を明記**しつつ、上記と同じ拒否をコードで行う（文書のみは不十分）。

## 完了条件

- **`as i64` によって負の値になりうる経路**をコード上で潰す。
- 可能なら **境界の単体テスト**（`i64::MAX as u64` は通す、`i64::MAX as u64 + 1` は拒否など）。
