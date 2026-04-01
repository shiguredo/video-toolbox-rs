# `test_supported_codecs` が「全 Mac で H.264/HEVC 対応」を前提にしている

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`src/lib.rs` の `test_supported_codecs` は **H.264 と HEVC のデコード・エンコードが必ず `supported` である**ことを `assert!` している。将来の macOS・仮想化環境・特殊構成では **失敗しうる**。CI の runner 種類を固定しない場合、**フレーク**の原因になる。

## 現状

- **場所**: `src/lib.rs` の `tests::test_supported_codecs`

## 望ましい対応の方向（案）

- **サポートされていること自体をテストの不変条件とする**なら、README または CI ドキュメントに **想定環境（例: Apple Silicon + 特定 OS 以上）**を明記する。
- または、**環境によってスキップ**する・**緩い検証**（例: エントリが存在する・件数が 4）に変更する。

## 解決の完了条件

- テストの前提と CI の期待が **文書またはテストコードのいずれかで一貫**していること。
