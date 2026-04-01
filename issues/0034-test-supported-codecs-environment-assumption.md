# `test_supported_codecs` の厳格な assert と実行環境の前提

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`src/lib.rs` の `test_supported_codecs` は **H.264 と HEVC のデコード・エンコードが必ず `supported` である**ことを `assert!` している。

**CI の事実（現状）**: `.github/workflows/ci.yml` の `test-video-toolbox` は **セルフホストランナー**に固定されており、`runs-on` は `labels: [self-hosted, macOS, ARM64]` である。**ランナー種類が毎回変わる GitHub ホステッド macOS とは異なり**、ここを主因としたフレークは想定しにくい。

**README の事実（現状）**: 動作要件に **`macOS (arm64)`** が明記されている。

したがって問題設定の中心は **「CI がランナー未固定でブレる」**ではなく、次のほうが実際に近い。

- **ローカル**で `cargo test` した開発者（例: Intel Mac、古い macOS、仮想化・特殊構成）が **同じ assert で落ちる**。
- **将来**、CI の運用変更（ランナー差し替え、ラベル変更、セルフホストの構成変更）や **Apple 側の VT ポリシー変化**で、**「必ず true」前提が崩れる**可能性。

issue 本文が CI フレーク中心のままだと、**修正先が README なのかテストなのかワークフロー／運用メモなのか**がぶれる。

## 現状

- **場所**: `src/lib.rs` の `tests::test_supported_codecs`
- **CI**: `ci.yml` の `test-video-toolbox`（上記ラベル）

## 望ましい対応の方向（案）

次のいずれか（または併用）を選び、**どれを主修正先にするか**を issue 完了時に明示する。

- **README** に、「この assert を前提にしたい実行環境（例: arm64・CI と同型のセルフホスト）」と、「それ以外では `cargo test` が落ちうる」旨を書く。
- **テスト**を、CI 事実と README に整合する前提のまま厳格に維持するか、**ローカルでは緩い検証**にするか、環境検出で早期終了するか、方針を決める（`#[ignore]` はプロジェクト規約で禁止のため、スキップ表現は既存の VP9/AV1 テストと同様のパターンに限る）。
- **`.github/workflows` またはチームの運用ドキュメント**に、「Video Toolbox 実テストはこのラベルのランナー前提」など **CI 側の前提を事実ベースで残す**。

## 解決の完了条件

- **現状の CI（セルフホスト `macOS` / `ARM64`）と README（`macOS (arm64)`）**を踏まえたうえで、テストの前提が **文書・テスト・ワークフロー／運用のいずれかで一貫**していること。
- 採った方針で、**主な修正先が README かテストか CI 記述か**が追試できること。
