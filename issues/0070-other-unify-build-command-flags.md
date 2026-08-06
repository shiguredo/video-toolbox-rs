# CI / Makefile / prek.toml のコマンドフラグを統一する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-unify-build-command-flags
- Polished: 2026-08-01

## 目的

CI / Makefile / prek.toml の clippy / test / fmt のコマンドフラグが三者で不一致になっている。`git commit --no-verify` で prek を迂回した場合、テストコードの clippy 警告が CI でも Makefile でも検出されない（CI / Makefile の clippy に `--all-targets` がない）。また CI の `--test-threads=1`（ハードウェアリソース競合回避）が Makefile と prek.toml に反映されていない。Makefile の fmt は `--check` がないため、実際にフォーマットを適用してしまう。

## 現状

| | clippy | test | fmt |
|---|---|---|---|
| CI | `--workspace`（`--all-targets` なし） | `--workspace -- --test-threads=1` | `--all --check` |
| Makefile | `--workspace`（`--all-targets` なし） | `--workspace`（`--test-threads=1` なし） | `--all`（`--check` なし、書き換え） |
| prek.toml | `--all-targets`（`--workspace` なし） | 裸の `cargo test` | `--all -- --check` |

## 設計方針

テストコードの clippy 警告を全経路で検出できるよう、CI に `--all-targets` を追加し、以下のコマンドに三者を統一する:

- clippy: `cargo clippy --workspace --all-targets -- -D warnings`
- test: `cargo test --workspace -- --test-threads=1`
- fmt: `cargo fmt --all -- --check`（prek.toml と既存 issue の完了条件で主流の形式に揃え、CI をこれに変更する）

### 変更対象

- `.github/workflows/ci.yml`: clippy に `--all-targets` を追加、fmt を `--all -- --check` に変更
- `Makefile`: clippy に `--all-targets` を追加、test に `-- --test-threads=1`、fmt に `-- --check` を追加
- `prek.toml`: clippy に `--workspace` を追加して `--all-targets` を維持、test に `--workspace -- --test-threads=1` を追加。clippy のコメントを「`--workspace` で全メンバー、`--all-targets` でテスト・example・ベンチも lint し、`-D warnings` で警告をエラーに昇格する」に更新し、test フックのコメントに `--test-threads=1`（ハードウェアリソース競合回避）の理由を追記する

ワークスペースはルートクレートと pbt クレートで構成され（issue 0057 で追加済み）、`--workspace` は両方を対象とする。

## 完了条件

- CI / Makefile / prek.toml の clippy / test / fmt のコマンドフラグが一致していること
- 既存の open issue が完了条件に記載している clippy コマンド（`cargo clippy --workspace -- -D warnings` または `cargo clippy --all-targets -- -D warnings`）が、統一後の `cargo clippy --workspace --all-targets -- -D warnings` に追従していること（実装時に既存 open issue の完了条件を更新する）
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0057: PBT インフラ導入で pbt クレートを追加し workspace 化する。0057 は closed 済みであり、その完了条件のコマンドは本 issue の実装で統一後の形式に追従させた
