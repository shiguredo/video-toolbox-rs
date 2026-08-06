# テストのログメッセージを日本語に統一する

- Created: 2026-07-30
- Completed: 2026-08-06
- Branch: feature/refactor-test-log-messages-japanese
- Polished: 2026-08-01

## 目的

AGENTS.md は「テストのログメッセージは全て日本語にすること」と定めているが、`tests/test_encoder.rs` と `tests/test_decoder.rs` のテストログメッセージ（`expect` / `expect_err` / `panic!` / `unreachable!` / カスタムメッセージ付き `assert!` / `assert_eq!` のメッセージ引数）が英語で書かれている。`src/encoder.rs` の `#[cfg(test)] mod tests` にも英語メッセージが残っている。一方 `tests/test_codec_info.rs` は日本語で、AGENTS.md の規約違反がファイル間の不整合として現れている。規約違反を解消して統一する。

## 現状

- `tests/test_encoder.rs`: `expect("encoder construction must succeed")` / `expect_err("invalid params must be rejected")` / `expect("results mutex poisoned")` / `panic!("unexpected encode callback error: {e}")` / `assert!(..., "window {i} produced {bytes} bytes, exceeding hard limit...")` 等が英語
- `tests/test_decoder.rs`: `expect("results mutex poisoned")` / `panic!("unexpected decode callback error: {e}")` / `unreachable!("expected I420 but got NV12")` / `assert_eq!(..., "frame {i}: width mismatch")` 等が英語
- `tests/test_codec_info.rs`: `expect("H.264 のコーデック情報が返ってくること")` 等で日本語
- `src/encoder.rs` の `#[cfg(test)] mod tests`: `expect_err("rescale should overflow")` が英語

対象外: `tests/test_error.rs` はメッセージなしの `assert!(expr)` 形式のみで、`contains("limit exceeded")` 等の文字列は `Error` の英語 Display 出力（`AGENTS.md` の「ログメッセージは全て英語にすること」に従う）の検証対象であるため、本 issue の対象外。

## 設計方針

- 対象はカスタムメッセージを持つ箇所のみ。メッセージなしの `assert!(expr)` / `assert_eq!(lhs, rhs)` のデフォルトメッセージ（Rust 標準の英語）は対象外とし、メッセージを追加しない
- メッセージ内の `{e}` / `{i}` / `{user_data}` 等のプレースホルダは維持する
- 対象ファイルを 1 つずつ確認しながら、英語メッセージを日本語に置換する

### 検証方法

対象は `tests/test_encoder.rs` / `tests/test_decoder.rs` / `tests/test_codec_info.rs` / `src/encoder.rs` の `#[cfg(test)] mod tests` に限定する。grep（`expect` / `expect_err` / `panic!` / `unreachable!` / カスタムメッセージ付き `assert!` / `assert_eq!` のメッセージ引数を検出）で拾えるカスタムメッセージが全て日本語であることを確認する。複数行にまたがる `assert!` / `assert_eq!` のカスタムメッセージは grep では拾えないため目視で確認する。プレースホルダと ASCII 識別子が混ざる箇所も目視で確認する。

## 完了条件

- `tests/test_encoder.rs` / `tests/test_decoder.rs` / `tests/test_codec_info.rs` / `src/encoder.rs` の `#[cfg(test)] mod tests` のテストログメッセージ（カスタムメッセージのみ）が日本語に統一されていること
- `tests/test_error.rs` は対象外（`Error` の Display 出力検証のため英語のまま）
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0051: `tests/test_encoder.rs` を変更対象とし、同一のテスト関数に触れる
- issue 0069: `tests/test_encoder.rs` / `tests/test_decoder.rs` にコメントを追加し、同一ファイルを変更対象とする。目的は別のため重複ではないが、差分衝突に注意する

## 解決方法

- `tests/test_encoder.rs` / `tests/test_decoder.rs` / `src/encoder.rs` のテストコードの
  カスタムメッセージ（`expect` / `expect_err` / `panic!` / `unreachable!` /
  カスタムメッセージ付き `assert!` / `assert_eq!`）を日本語に統一した
  - プレースホルダ（`{e}` / `{i}` / `{user_data}` / `{bytes}` / `{total}` 等）は維持
  - メッセージなしの `assert!` / `assert_eq!` にはメッセージを追加していない
- `tests/test_codec_info.rs` は変更前から日本語だったため変更なし
- 対象外の確認
  - `tests/test_error.rs` は `Error` の Display 出力検証（英語）のため変更なし
  - ライブラリのログ検証文字列（`output_callback_h264: user handler panicked` 等）は
    ログが英語のため英語のまま
- テストハンドラ内の `panic!("テストハンドラが意図的に panic した")` は日本語に統一した。
  このメッセージは `catch_user_panic` のエラーログ（`"{callback_name}: user handler panicked: {message}"`）
  に埋め込まれるが、メッセージはテストコード出自の値であり、ライブラリのログフォーマット
  （"user handler panicked" 等）は英語のままとする
- `CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記した
- 完了条件の確認結果
  - 対象ファイルのカスタムメッセージが日本語に統一されていること（grep + 目視で確認）
  - `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` /
    `cargo fmt --all -- --check` がすべて通ることを確認
