# `Encoder::reconfigure` 関連 rustdoc / README を実装と整合させる

- Priority: High
- Created: 2026-05-14
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-encoder-reconfigure-docs
- Updated: 2026-07-16

## 目的

`Encoder::reconfigure` 関連の説明が rustdoc 3 ヶ所 + README 3 ヶ所に分散・重複しており、
一部が実装の現状 (`ReconfigureParams` 化と `data_rate_limits` 追加、コミット bc6ce71) に
追随できていない。残っている不整合は以下の 3 点。

1. `Encoder::config` の rustdoc (`src/encoder.rs:378-380`) が「動的に更新され得るのは
   `average_bitrate` / `fps_numerator` / `fps_denominator` / `data_rate_limits` の 4 項目のみ」と
   `EncoderConfig` のフィールド名で列挙している。戻り値 `EncoderConfig` のどのフィールドが
   変わり得るかという説明としては実装と一致しているが、`ReconfigureParams` のフィールド名
   (`expected_frame_rate`) との対応 (`expected_frame_rate` 指定で `fps_numerator` / `fps_denominator`
   が書き換わる) は利用者が自分で突き合わせる必要がある
2. README に `fps_denominator = 1` 正規化 (分数 fps は保持されない) の記載が無い。また
   「分数 fps を保持したい場合は `Encoder` を作り直す」という明示は rustdoc / README の
   どこにも無い (rustdoc の「作り直す」は解像度・コーデック・ピクセルフォーマットの文脈のみ)
3. 「reconfigure できる項目 / できない項目」の説明が複数箇所に重複しており、
   項目が増えるたびに追随漏れが起きる (現に README「特徴」節は `data_rate_limits` 未反映)

公開 API のドキュメントは利用者が真に依存する契約なので、分散による追随漏れを構造的に防ぐ。

## 優先度根拠

- 当初 High とした根拠 2 点 (rustdoc「3 項目」表記が `ReconfigureParams` の実フィールドと乖離、
  fps 正規化が rustdoc / README のどこにも書かれていない) は bc6ce71 で解消済み
- 残タスクは説明の集約と README への記載追加であり、実装挙動の誤解を招く度合いは当初より低い
- 優先度を Medium へ引き下げる余地がある (判断はレビュー時に行う)

## 現状

### 解消済み (bc6ce71、issue 0043 / 0054 の対応に含まれる)

- `Encoder::reconfigure` の rustdoc から「`fps_numerator` / `fps_denominator` の 3 項目」表記は
  消滅し、`expected_frame_rate` ベースの記述に更新済み
- `fps_numerator` / `fps_denominator` が `expected_frame_rate / 1` に正規化されること
  (分数 fps 非保持)、切り上げ再スケールによる PTS 前倒しドリフトは
  `Encoder::reconfigure` の rustdoc (`src/encoder.rs:394-399`) に明記済み
- `ReconfigureParams::expected_frame_rate` の rustdoc (`src/encoder.rs:169-172`) は
  `Encoder::reconfigure` の rustdoc への参照リンクで完結している
- README のコード例 (`README.md:294-304`) は `..Default::default()` 付きでコンパイル可能な形に
  修正済み。「動的設定更新」節 (`README.md:290`) は「`average_bitrate` / `expected_frame_rate` /
  `data_rate_limits` の 3 つ」と実フィールド名で整合済み。比較表 (`README.md:343-348`) も
  `ReconfigureParams` 名称で整合済み

### 残存 1: `Encoder::config` rustdoc の列挙

`src/encoder.rs:378-380`:

```
/// [`Encoder::reconfigure`] 経由で動的に更新され得るのは `average_bitrate` /
/// `fps_numerator` / `fps_denominator` / `data_rate_limits` の 4 項目のみで、
/// その他のフィールドは [`Encoder::new`] で渡した初期値のまま保持される。
```

実装 (`src/encoder.rs:456-471` で実際にこの 4 フィールドを更新) とは一致しているため誤りではないが、
`ReconfigureParams` のフィールド名との対応が示されておらず、重複の一因にもなっている。

### 残存 2: README の正規化未記載

- 正規化コード本体は `src/encoder.rs:460-462` (`self.config.fps_denominator = 1`)
- これにより `encode()` / `encode_pixel_buffer()` 内の PTS 進行幅
  (`next_input_pts += fps_denominator`、`src/encoder.rs:1008-1010` / `1092-1094`) が変わる
- README の「動的設定更新」節 (`README.md:285-304`) にはこの正規化挙動の記載が無い
  (`grep -n "正規化" README.md` は 0 件)

### 残存 3: 説明の重複 (rustdoc 3 ヶ所 + README 3 ヶ所)

「動的更新可能項目 / 不可能項目」の説明が以下に分散している
(当初の「4 + 2 = 6 ヶ所」という数え方は列挙と合っていなかったため数え直した):

- `ReconfigureParams` rustdoc (`src/encoder.rs:154-163`)
- `Encoder::config` rustdoc (`src/encoder.rs:378-380`)
- `Encoder::reconfigure` rustdoc (`src/encoder.rs:385-401`)
- README「特徴」節 (`README.md:33-34`) — `data_rate_limits` 未反映のまま
- README「動的設定更新」節 (`README.md:285-304`)
- README「まとめ」比較表 (`README.md:341-348`)

特に「解像度・コーデック・ピクセルフォーマットは `Encoder` を作り直す」という説明は
`src/encoder.rs:158-159` / `src/encoder.rs:401` / `README.md:34` / `README.md:291-292` /
`README.md:348` の 5 ヶ所で重複している。

## 設計方針

- 「reconfigure できる項目 / できない項目」の本体説明を 1 ヶ所に集約し、他は参照リンクにする
  - 当初案は `ReconfigureParams` rustdoc への集約だったが、bc6ce71 後の現状は
    `Encoder::reconfigure` rustdoc が本体でフィールド rustdoc からリンクする逆構造になっている。
    正規化・再スケールはメソッド呼び出しの副作用なのでメソッド側を本体とする現状構造にも
    合理性があり、集約先は実装時にどちらかへ決定する
- `Encoder::config` rustdoc の 4 項目列挙は、`ReconfigureParams` フィールドとの対応
  (`expected_frame_rate` 指定で `fps_numerator` / `fps_denominator` が書き換わる) が分かる形に
  するか、集約先への参照リンクに縮小する
- README は「動的設定更新」節を本体とし、「特徴」節は `data_rate_limits` を含む現仕様に追随させる
- `fps_denominator = 1` 正規化 (分数 fps 非保持) と「分数 fps を保持したい場合は `Encoder` を
  作り直す」を README「動的設定更新」節と rustdoc に明示する
- 注: issue 0053 (`src/encoder.rs` のモジュール分割) が実施されると本 issue の行番号参照は
  無効になる。着手時は行番号ではなくシンボル名で対象を特定すること

## 完了条件

- 「reconfigure できる項目 / できない項目」の本体説明が 1 ヶ所だけにあり、他は参照で完結する
- `Encoder::config` rustdoc が `ReconfigureParams` フィールドとの対応を、利用者が自分で
  突き合わせることなく理解できる形になっている
- README「動的設定更新」節に `fps_denominator = 1` への正規化 (分数 fps 非保持) と
  「分数 fps を保持したい場合は `Encoder` を作り直す」が明示されている
- README「特徴」節が `data_rate_limits` を含む現仕様と整合している
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

- 集約先に決めた rustdoc (`ReconfigureParams` または `Encoder::reconfigure`) へ本体説明を移し、
  残り 2 ヶ所の rustdoc は `[...]` 参照に書き換える
- `Encoder::config` rustdoc (`src/encoder.rs:373-381`) の列挙を対応関係の明記または
  参照リンク化で整理する
- `README.md:285-304` に正規化挙動と「分数 fps は `Encoder` 作り直し」の記述を追加する
- `README.md:33-34` の「特徴」節を現仕様 (`data_rate_limits` を含む) に追随させるか、
  個別項目の列挙をやめて「動的設定更新」節への誘導だけにする
