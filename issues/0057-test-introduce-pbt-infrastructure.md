# PBT (proptest) 基盤を導入し検証系純関数と PTS 再スケールを PBT でカバーする

- Priority: Medium
- Created: 2026-07-16
- Completed:
- Model: Fable 5
- Branch: feature/add-pbt-infrastructure
- Polished: {YYYY-MM-DD}

## 目的

AGENTS.md は「PBT(Property-Based Testing) や Fuzzing でテストを行うこと」「PBT は proptest を使うこと」「PBT のファイル名は `pbt/tests/prop_<module>.rs` とする」と規定しているが、本リポジトリには `pbt/` パッケージが存在しない。

さらに `Makefile:12-17` には `pbt` / `pbt-with-cover` ターゲットが定義済みで `cargo test -p pbt` を呼ぶが、`Cargo.toml` は単一クレート構成 (workspace 未定義) のため `-p pbt` は解決できず、このターゲットは実行できない死んだ定義になっている。

本クレートは実ハードウェア (Video Toolbox) を叩く FFI クレートのため PBT の適用範囲は純ロジックに限られるが、その純ロジックが直近の変更で増えている:

- `validate_average_bitrate` (`src/encoder.rs:185-199`)
- `validate_data_rate_limits` (`src/encoder.rs:205-233`)
- `validate_fps_numerator` (`src/encoder.rs:236-250`)
- `validate_expected_frame_rate` (`src/encoder.rs:253-267`)
- `validate_video_dimensions_for_toolbox` (`src/types.rs:6-33`)
- `validate_frame_data` / `frame_byte_len_checked` (`src/encoder.rs:832-893`)
- `Encoder::reconfigure` 内の PTS 再スケール演算 (`src/encoder.rs:411-424` の `div_ceil` による切り上げ)

特に PTS 再スケールは「物理時間としての単調性 (rescaled / new_timescale >= old_pts / old_timescale)」「切り上げの最小性」という PBT 向きのプロパティを持つのに、`Encoder` のメソッド内に埋め込まれているため実機セッション無しではテストできず、現状は example ベースの内部テスト 3 本 (`src/encoder.rs:1599-1683`) しか無い。

## 優先度根拠

- AGENTS.md のテスト規約 (PBT / Fuzzing) と実態が乖離しており、Makefile には実行不能なターゲットが残っている
- 検証系純関数は今後も増える傾向 (直近で `validate_data_rate_limits` が追加) にあり、基盤が無いと example ベースの単体テストが増殖し続ける
- 機能のバグではないため High ではなく Medium

## 現状

- `Cargo.toml`: 単一クレート。`[workspace]` 定義無し
- `pbt/` ディレクトリ: 存在しない
- `Makefile:12-17`: `pbt:` → `cargo test -p pbt`、`pbt-with-cover:` → `cargo llvm-cov -p pbt --tests` (いずれも実行不能)
- 検証系純関数のテストは `tests/test_encoder.rs` の example ベース単体テストのみ

## 設計方針

1. ルート `Cargo.toml` に `[workspace]` を定義し、`pbt/` をワークスペースメンバーとして追加する (`pbt` は publish 対象外の内部テスト専用クレート。`publish = false` を付ける)
2. `pbt/Cargo.toml` に `proptest` を dev-dependency として追加し、本体クレートへ path 依存する
3. AGENTS.md の命名規則に従い `pbt/tests/prop_encoder.rs` / `pbt/tests/prop_types.rs` を作成する
4. PTS 再スケール演算を `fn rescale_pts(old_pts: i64, old_timescale: u32, new_timescale: u32) -> Result<i64, Error>` のような純関数へ抽出して `reconfigure` から呼ぶ (PBT から到達可能にするため。可視性は `pub(crate)` とし、pbt クレートからの参照方法は cfg か公開度で調整する)
5. 検証するプロパティの例:
   - `rescale_pts`: 単調性 (old_pts が増えれば rescaled も減らない)、物理時間の非逆行 (rescaled / new >= old_pts / old)、切り上げの最小性、`i64::MAX` 超過時に必ず `Err`
   - `validate_*`: 受理域と拒否域の境界 (0 拒否、`i32::MAX` / `i64::MAX` 境界、`DataRateLimit` の個数 0〜2)
   - `validate_frame_data`: プレーン長が期待値以上のとき受理、1 バイトでも不足すれば拒否
6. Makefile の `pbt` / `pbt-with-cover` ターゲットがそのまま動くことを確認する

なお AGENTS.md は Fuzzing (cargo-fuzz) も規定しているが、fuzz 基盤は入力パース系が主対象であり本クレートには適用対象が薄いため、本 issue では PBT のみを扱う (Fuzzing は必要になった時点で別 issue とする)。

## 完了条件

- `pbt/` ワークスペースメンバーが存在し、`make pbt` (`cargo test -p pbt`) が通る
- PTS 再スケールが純関数として抽出され、既存の内部テスト (`src/encoder.rs` の mod tests) と挙動が一致する
- 上記「検証するプロパティの例」に挙げた検証系純関数の PBT が `pbt/tests/prop_encoder.rs` / `pbt/tests/prop_types.rs` に存在する
- 既存の example ベース単体テストのうち PBT で完全に代替できるものは削除されている (AGENTS.md「PBT でカバーできるものを単体テストで書かない」)
- `cargo test --workspace` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo fmt --all -- --check` が通る
