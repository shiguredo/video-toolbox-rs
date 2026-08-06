# PBT (proptest) 基盤を導入し検証系ロジックの拒否域を PBT でカバーする

- Priority: Medium
- Created: 2026-07-16
- Updated: 2026-07-21
- Completed: 2026-08-06
- Model: Fable 5
- Branch: feature/add-pbt-infrastructure
- Polished: 2026-07-31

## 目的

shiguredo-rust スキルは「PBT(Property-Based Testing) や Fuzzing でテストを行うこと」「PBT は proptest を使うこと」「PBT のファイル名は `pbt/tests/prop_<module>.rs` とする」と規定しているが、本リポジトリには `pbt/` パッケージが存在しない。

さらに `Makefile` には `pbt` / `pbt-with-cover` ターゲットが定義済みで `cargo test -p pbt` を呼ぶが、`Cargo.toml` は単一クレート構成 (workspace 未定義) のため `-p pbt` は解決できず、このターゲットは実行できない死んだ定義になっている。

本クレートは実ハードウェア (Video Toolbox) を叩く FFI クレートのため、PBT の適用範囲は公開 API 経由で到達可能な純ロジックに限られる。具体的には「不正な入力（境界をまたぐ値）を `Encoder::new` / `Encoder::reconfigure` が必ず `Err` で拒否する」という性質の検証が対象になる。`Encoder::new` の拒否域は、拒否される入力が `validate_config` で弾かれて `create_compression_session` に到達しないため、実機セッションなしで PBT を実行できる。`Encoder::reconfigure` の拒否域は、`Encoder` インスタンスの構築（実 FFI セッション生成）が前提になるため、検証対象の `Encoder` を 1 つ構築して使い回す構成になる（CI はセルフホスト macOS のため実行可能）。

## 優先度根拠

- shiguredo-rust スキルのテスト規約 (PBT / Fuzzing) と実態が乖離しており、Makefile には実行不能なターゲットが残っている
- 検証系ロジックは今後も増える傾向 (直近で `validate_data_rate_limits` が追加) にあり、基盤が無いと単体テストが増殖し続ける
- 機能のバグではないため High ではなく Medium

## 現状

- `Cargo.toml`: 単一クレート。`[workspace]` 定義無し
- `pbt/` ディレクトリ: 存在しない
- `Makefile`: `pbt:` → `cargo test -p pbt`、`pbt-with-cover:` → `cargo llvm-cov -p pbt --tests` (いずれも実行不能)
- 検証系ロジックのテストは `tests/test_encoder.rs` の単体テストのみ

## 設計方針

1. ルート `Cargo.toml` に `[workspace]` を定義し、`pbt/` をワークスペースメンバーとして追加する (`pbt` は publish 対象外の内部テスト専用クレート。`publish = false` を付ける)
2. `pbt/Cargo.toml` に `proptest` を dev-dependency として追加し、本体クレートへ path 依存する
3. 命名規則に従い `pbt/tests/prop_encoder.rs` を作成する (`prop_types.rs` は作成しない。`src/types.rs` に対応する PBT 対象の公開 API が無いため。幅・高さの拒否域は `Encoder::new` 経由で `prop_encoder.rs` で検証する)
4. 検証するプロパティ（**公開 API 経由で到達可能な範囲に限定する**）:
   - `Encoder::new` が不正な `EncoderConfig`（`width` 0 / `height` 0 / `fps_numerator` 0 / `fps_numerator` が `i32::MAX` 超え / `fps_denominator` 0 / `average_bitrate` 0 / `average_bitrate` が `i64::MAX` 超え / `data_rate_limits` が 3 個以上等）を必ず `Err` で拒否する
   - `Encoder::reconfigure` が不正な `ReconfigureParams`（`average_bitrate` 0 / `expected_frame_rate` 0 / `expected_frame_rate` が `i32::MAX` 超え / 不正な `data_rate_limits` 等）を必ず `Err` で拒否する
   - 入力の生成は「不正フィールドを 1 個だけ固定し、残りのフィールドは有効範囲から生成する」戦略にする（`proptest::prop_oneof` で各不正フィールドを 1 ケースずつ用意する）。任意値ベースの naive な生成にすると、有効な `EncoderConfig` が生成されて実 FFI セッションが作られたり、検証ロジックが壊れてもテストが通ったりするため
   - 拒否される入力は `validate_config` / `validate_reconfigure_params` で弾かれる。`Encoder::new` の拒否域は `create_compression_session` に到達しないため実機セッションなしで実行できる。`Encoder::reconfigure` の拒否域は検証対象の `Encoder` を 1 つ構築して使い回す
   - 受理域（検証を通過する入力）は実 FFI セッション生成を伴うため PBT の対象外とする。境界値の受理・拒否は既存の単体テスト（`encoder_rejects_*` / `reconfigure_rejects_*` 等）がカバーする
5. **PTS 再スケールの PBT は対象外とする**: `next_input_pts` は `Encoder` の private フィールドであり、公開 API 経由では到達不能（`reconfigure` は実 FFI の `VTSessionSetProperties` を呼ぶため PBT に不向き）。切り上げ (`div_ceil`) の回帰は既存の内部テスト（`src/encoder.rs` の `#[cfg(test)] mod tests` の `reconfigure_rescales_next_input_pts_on_frame_rate_change`）で保護されている
6. Makefile の `pbt` / `pbt-with-cover` ターゲットがそのまま動くことを確認する。あわせて `.PHONY` の誤記（`pbt-cover` → 実ターゲットは `pbt-with-cover`、`fuzz` → 実ターゲットは `fuzzing`）を修正する

なお shiguredo-rust スキルは Fuzzing (cargo-fuzz) も規定しているが、fuzz 基盤は入力パース系が主対象であり本クレートには適用対象が薄いため、本 issue では PBT のみを扱う (Fuzzing は必要になった時点で別 issue とする)。

## 関連 issue

- issue 0046（fps バリデータ統合）: `validate_fps_numerator` / `validate_expected_frame_rate` の拒否域は本 issue の PBT 対象。どちらを先に実施しても成立する（先に 0046 を実施する場合は、PBT は統合後の関数の拒否域を対象にする）
- issue 0049（PTS 再スケールの切り下げ方向テスト）: 本 issue では PTS 再スケールの PBT を対象外としたため、0049 が提案する単体テスト追加とは独立に扱う（0049 の扱いの判断は本 issue とは別）
- issue 0076（reconfigure の data_rate_limits がエンコード開始後に実効しない。バグ issue）: 本 issue の PBT（拒否域の検証）は config レベルの検証であり、0076 の実効性問題とは独立

## 完了条件

- `pbt/` ワークスペースメンバーが存在し、`make pbt` (`cargo test -p pbt`) と `make pbt-with-cover` (`cargo llvm-cov -p pbt --tests`) が通る
- `Encoder::new` / `Encoder::reconfigure` が不正入力を必ず `Err` で拒否することを検証する PBT が `pbt/tests/prop_encoder.rs` に存在する（拒否域のみ。「不正フィールドを 1 個だけ固定し、残りは有効範囲から生成する」戦略。受理域は実 FFI セッション依存のため対象外）
- 既存の単体テストのうち、追加する PBT で完全に代替できるものは削除されている（既存の `encoder_rejects_*` / `reconfigure_rejects_*` テストは field / reason 文字列まで検証しており、PBT の性質検証では代替されない。`src/encoder.rs` の内部テスト 3 本も FFI 経路・制御フロー・失敗時の状態不変を検証しており、PBT では代替されない）
- `CHANGES.md` の `## develop` に `[UPDATE]` としてエントリを追記する（`### misc` サブセクション）
- `cargo test --workspace` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る（clippy / fmt のフラグは issue 0070 の統一後に追従する）

## 解決方法

PBT (proptest) 基盤を導入した。

- ルート `Cargo.toml` に `[workspace] members = ["pbt"]` を追加し、`pbt/` を publish 対象外 (`publish = false`) の内部テスト専用クレートとして追加した
- `pbt/Cargo.toml` に `proptest = "1.11"` を dev-dependency として追加し、本体クレートへ path 依存した (`rust-version = "1.93"` も明記)
- `pbt/tests/prop_encoder.rs` を作成し、`Encoder::new` / `Encoder::reconfigure` の拒否域を PBT で検証した
  - 「不正フィールドを 1 個だけ固定し、残りは有効範囲から生成する」戦略 (`prop_oneof` で各拒否パスを列挙)
  - `validate_config` の全 15 パスと `validate_reconfigure_params` の全 8 パスを 1:1 で網羅
  - 拒否時は必ず `Error::InvalidConfig` を返すこと、reconfigure 拒否時は `config()` が不変であることを検証
  - reconfigure の検証は実 FFI セッション 1 つを `RefCell` で包んで使い回す
- `Makefile` の `.PHONY` の誤記 (`pbt-cover` → `pbt-with-cover`、`fuzz` → `fuzzing`) を修正し、`make pbt` / `make pbt-with-cover` が実行できることを確認した
- `CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記した

`cargo test --workspace` は全 48 テスト (pbt 2 件を含む) がパスする。
