# CI / Makefile / prek.toml のコマンドフラグを統一する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-unify-build-command-flags
- Polished: {YYYY-MM-DD}

## 目的

CI / Makefile / prek.toml の clippy と test のコマンドフラグが三者で不一致になっている。`git commit --no-verify` で prek を迂回した場合、テストコードの clippy 警告が CI でも Makefile でも検出されない。また CI の `--test-threads=1`（ハードウェアリソース競合回避）が Makefile と prek.toml に反映されていない。

## 現状

| | clippy | test |
|---|---|---|
| CI | `--workspace`（`--all-targets` なし） | `--workspace -- --test-threads=1` |
| Makefile | `--workspace`（`--all-targets` なし） | `--workspace`（`--test-threads=1` なし） |
| prek.toml | `--all-targets`（`--workspace` なし） | 裸の `cargo test` |

## 設計方針

CI を正とし、Makefile と prek.toml を CI と同じフラグに揃える。

## 完了条件

CI / Makefile / prek.toml の clippy と test のコマンドフラグが一致していること。
