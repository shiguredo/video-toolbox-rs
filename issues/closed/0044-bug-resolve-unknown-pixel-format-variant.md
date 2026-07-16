# `Error::UnknownPixelFormat` の利用箇所欠落と CHANGES.md 未記載を解消する

- Priority: High
- Created: 2026-05-14
- Completed: 2026-07-16
- Model: Opus 4.7
- Branch: feature/fix-error-unknown-pixel-format

## 目的

公開 enum `Error` に `UnknownPixelFormat { expected, fourcc }` バリアントが追加されているが、`grep -rn "UnknownPixelFormat" src/ tests/` の結果は `src/error.rs` 自身の定義 (`src/error.rs:51-57`) と `Display` 実装 (`src/error.rs:113-118`) のみで、生成箇所が一切無い。本来の発生点である `Encoder::encode_pixel_buffer` の未知 FourCC 分岐 (`src/encoder.rs:914-928` 付近) は依然として `PixelFormatMismatch` に「期待値の逆をハック的に詰めた `actual`」を返している。

加えて、`pub enum Error` には `#[non_exhaustive]` が付いていないため、外部の網羅 `match` を破壊する後方非互換変更でありながら `CHANGES.md` の `## develop` セクションに該当エントリが無い。

この dead variant の存在 + リリースノート不整合を放置すると、ユーザーが本物の不一致と未知 FourCC を診断上区別できず、公開 API の意味が壊れる。

## 優先度根拠

- 公開 enum へのバリアント追加 = 後方非互換変更で、外部利用者の `match` が壊れるリスクがある
- 直近コミット `36b0348` のメッセージには「未知フォーマットには `Error::UnknownPixelFormat` を返す」と書かれており、実装と意図が乖離している
- CHANGES.md の未記載は CLAUDE.md の「変更点とリリースノートの整合性を確認すること」に反する
- 設計の自己矛盾なので最優先 (High) で解消すべき

## 現状

- `src/error.rs:51-57`: `UnknownPixelFormat { expected: PixelFormat, fourcc: u32 }` バリアントが定義されている
- `src/error.rs:113-118`: `Display` 実装が `"unknown pixel format: encoder expects ... FourCC=0x..."` を出力する形で揃っている
- `src/encoder.rs:914-928` 付近: 未知 FourCC が来た場合の分岐は以下のような実装になっている

  ```rust
  _ => {
      // 未知のフォーマットは I420 でも Nv12 でもないので、
      // どちらを actual にしても不一致になる。期待値の逆を返す。
      let actual = match self.config.pixel_format {
          PixelFormat::I420 => PixelFormat::Nv12,
          PixelFormat::Nv12 => PixelFormat::I420,
      };
      return Err(Error::PixelFormatMismatch { expected, actual });
  }
  ```

- `tests/test_error.rs` には `UnknownPixelFormat` の `Display` 出力検証テストは存在しない
- `CHANGES.md` の `## develop` セクションには `[ADD]` / `[CHANGE]` どちらの形でも `UnknownPixelFormat` 追加の記載が無い

## 設計方針

以下のいずれかの方針を選ぶ。

### 方針 A: バリアントを採用する

- `Encoder::encode_pixel_buffer` の未知 FourCC 分岐を `Err(Error::UnknownPixelFormat { expected: self.config.pixel_format, fourcc: format_type })` に差し替える
- 「期待値の逆を返す」というコメントとロジックは削除する
- `CHANGES.md` に `[CHANGE] Error に UnknownPixelFormat バリアントを追加する` を追記する
- `tests/test_error.rs` に `Display` 出力検証テストを追加する

### 方針 B: バリアントを撤回する

- `src/error.rs` の `UnknownPixelFormat` 定義 (`src/error.rs:51-57`) と `Display` 分岐 (`src/error.rs:113-118`) を削除する
- 既存の `PixelFormatMismatch` 「期待値の逆を返す」ハックは別 issue で再検討する (将来も診断不能のままにするかどうか)

兄弟ライブラリで同等のエラーがどう扱われているか確認した上で方針を決定する。設計判断が必要な場合は `issues/pending/` に移動して保留する。

## 完了条件

- `grep -rn "UnknownPixelFormat" src/ tests/` の結果が、定義と利用箇所が論理的に揃った状態である (方針 A) か、すべての参照が消滅している (方針 B)
- `CHANGES.md` の `## develop` セクションに `UnknownPixelFormat` 関連のエントリが追加されている (方針 A の場合) か、変更自体がブランチから消えている (方針 B の場合)
- 方針 A の場合: `tests/test_error.rs` に `Display` 出力検証テストが追加されている
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

方針 A (バリアントを採用する) で対応した。バリアントと `Display` 実装は既に存在し、
追加コミット (`36b0348`) の意図も「未知フォーマットには `UnknownPixelFormat` を返す」だったため、
撤回 (方針 B) ではなく利用箇所の欠落を埋める方向とした。

- `src/encoder.rs` の `encode_pixel_buffer` の未知 FourCC 分岐を
  `Err(Error::UnknownPixelFormat { expected: self.config.pixel_format, fourcc: format_type })` に置換し、
  「期待値の逆を返す」ハックとそのコメントを削除した
- `CHANGES.md` の `## develop` に `[CHANGE] Error に UnknownPixelFormat バリアントを追加する` を追記し、
  網羅 `match` への影響 (`#[non_exhaustive]` ではないため分岐追加が必要) を明記した
- `tests/test_error.rs` に `error_display_unknown_pixel_format` を追加し、
  `fourcc = 0x30323449` (FourCC `I420`) のときの表示が `0x30323449` と `expected` の
  Debug 表示 (`Nv12`) を含むことを確認した
