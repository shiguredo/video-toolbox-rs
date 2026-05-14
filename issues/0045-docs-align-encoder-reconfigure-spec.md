# `Encoder::reconfigure` 関連 rustdoc / README を実装と整合させる

- Priority: High
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-encoder-reconfigure-docs

## 目的

`Encoder::reconfigure` と `Encoder::config` の rustdoc が、`ReconfigureParams` の実体と一致していない。具体的には以下の 2 点。

1. rustdoc に「動的に更新され得るのは `average_bitrate` / `fps_numerator` / `fps_denominator` の 3 項目のみ」と書かれているが、`ReconfigureParams` の公開フィールドは `average_bitrate: Option<u64>` と `expected_frame_rate: Option<u32>` の 2 項目のみ。`fps_numerator` / `fps_denominator` は内部正規化の結果書き換わるだけで、ユーザーが直接渡すフィールドではない。
2. `reconfigure(expected_frame_rate: Some(N))` 呼び出しで `fps_denominator` が暗黙のうちに `1` に正規化される (`src/encoder.rs:353-354`)。これは初期で `30000/1001` 等の分数 fps を使っていたユーザーに精度劣化を引き起こすが、rustdoc / README で明示されていない。

さらに、同じ「reconfigure できる項目 / できない項目」の説明が rustdoc 4 ヶ所 + README 2 ヶ所に重複している。

公開 API のドキュメントは利用者が真に依存する契約なので、実装との乖離は最優先で解消する。

## 優先度根拠

- 公開 API rustdoc は `cargo doc` で利用者に直接届く
- 「3 項目」と書いてあるのに `ReconfigureParams` に対応するフィールドが存在せず、ユーザーが構築時に混乱する
- 分数 fps の正規化挙動が明示されないと、`Encoder` の利用者が PTS 計算で誤った仮定を置く
- 実装挙動の誤解を招く High リスク

## 現状

### 不整合 1: フィールド名のミスマッチ

- `src/encoder.rs:277-279` (`Encoder::config` の rustdoc):

  ```
  /// `Encoder::reconfigure` 経由で動的に更新され得るのは `average_bitrate` /
  /// `fps_numerator` / `fps_denominator` の 3 項目のみで、その他のフィールドは
  /// `Encoder::new` で渡した初期値のまま保持される。
  ```

- `src/encoder.rs:285-297` (`Encoder::reconfigure` の rustdoc): 同様に「3 項目」表現を使用
- 実際の `ReconfigureParams` 定義 (`src/encoder.rs:140-148`): フィールドは `average_bitrate` と `expected_frame_rate` の 2 項目のみ

### 不整合 2: `fps_denominator = 1` 正規化の不告知

- `src/encoder.rs:353-354` で `expected_frame_rate` 指定時に `self.config.fps_denominator = 1` を強制
- これにより `encode()` 内の PTS 進行幅 (`next_input_pts += fps_denominator`) が変わる (`src/encoder.rs:881-886` 付近)
- README.md (`README.md:281-302`) や rustdoc にこの正規化挙動が書かれていない

### 不整合 3: rustdoc 重複

- 「動的更新可能項目 / 不可能項目」の説明が以下 4 + 2 = 6 ヶ所に散らばっている:
  - `ReconfigureParams` rustdoc (`src/encoder.rs:130-138`)
  - `Encoder::config` rustdoc (`src/encoder.rs:272-279`)
  - `Encoder::reconfigure` rustdoc (`src/encoder.rs:285-297`)
  - README.md 「特徴」節 (`README.md:33-35`)
  - README.md 「動的設定更新」節 (`README.md:281-302`)

## 設計方針

- 「reconfigure できる項目 / できない項目」の本体説明を `ReconfigureParams` rustdoc 1 ヶ所に集約する
- `Encoder::config` / `Encoder::reconfigure` の rustdoc は「詳細は [`ReconfigureParams`] を参照」とリンクするだけにする
- README.md は「特徴」節を 1 行に圧縮し、本文は「動的設定更新」節に集約する
- `fps_denominator` の `1` 正規化挙動は `Encoder::reconfigure` rustdoc と `ReconfigureParams::expected_frame_rate` rustdoc に明示する
- フィールド名「`average_bitrate` / `fps_numerator` / `fps_denominator` の 3 項目」は「`ReconfigureParams` で受ける `average_bitrate` と `expected_frame_rate` の 2 項目」に書き換える

## 完了条件

- `cargo doc --no-deps` の出力で `Encoder::reconfigure` / `Encoder::config` / `ReconfigureParams` の説明が `ReconfigureParams` のフィールド名と一致する
- 「`fps_denominator = 1` への正規化」「分数 fps を保持したい場合は `Encoder` を作り直す」が明示されている
- 同じ説明が 1 ヶ所だけにあり、他は参照で完結する
- README.md の重複が解消されている
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

- `src/encoder.rs:130-148` の `ReconfigureParams` rustdoc に「`fps_denominator` は内部で `1` に正規化される」を追記
- `src/encoder.rs:272-282` の `Encoder::config` rustdoc を縮小し `ReconfigureParams` へのリンクのみにする
- `src/encoder.rs:283-298` の `Encoder::reconfigure` rustdoc から「`fps_numerator` / `fps_denominator` の 3 項目」表記を削除し、「`average_bitrate` と `expected_frame_rate`」に書き換える
- `README.md:33-35` を 1 行に圧縮し、`README.md:281-302` の本文と重複している箇所を削る
- `README.md:339-345` の比較表 (`reconfigure()` の引数欄) も `ReconfigureParams` 名称で整合させる
