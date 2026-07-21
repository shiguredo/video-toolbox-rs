# 冗長コメント・`#[allow(dead_code)]` を整理する

- Priority: Low
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/fix-cleanup-misc-cruft

## 目的

レビューで指摘された雑多な「削るべき記述」を 1 つの issue にまとめて消化する。個別に issue を切るほどの分量ではないが、放置するとノイズが累積する。

具体的には以下の 2 件:

1. `src/encoder.rs` 内部テスト導入コメント (`mod tests` 直下) の過剰部分
2. `Encoder<H>` の `handler` フィールドに付いている `#[allow(dead_code)]` を `#[expect(dead_code, reason = "...")]` に置換

（当初提案に含めていた「`CHANGES.md` の空 `### misc` セクション除去」は、その後 `### misc` に `[UPDATE] Encoder::reconfigure 関連の rustdoc を整理する` エントリが入って空でなくなったため、本 issue の対象から外している。）

## 優先度根拠

- いずれも機能に影響しない掃除
- 緊急度は低いが、繰り返しレビューで指摘される類の項目を残し続けるとコードの信号雑音比が下がる
- Low

## 現状

### 1. `src/encoder.rs:1547-1551` の内部テスト導入コメント

```rust
#[cfg(test)]
mod tests {
    //! `Encoder::reconfigure` の内部状態 (`next_input_pts`) を直接確認するためのテスト。
    //! `tests/test_encoder.rs` からは到達できない private フィールド検証だけをここに置く。
    //! 通常系の検証は `tests/test_encoder.rs` 側にある。
```

3 行のうち 1 行目は `#[test]` 関数名から自明、3 行目は逆方向の参照で不要。2 行目だけが本質情報 (なぜ private フィールド検証だけここに置くのか)。

### 2. `Encoder<H>` の `handler` フィールド

`src/encoder.rs:343-346`:

```rust
pub struct Encoder<H: EncodeHandler> {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    // FFI の outputCallbackRefCon にこの Box の中身ポインタを渡しているため、
    // Encoder の生存期間中は保持し続ける必要がある。Rust 側からは直接参照しない。
    #[allow(dead_code)]
    handler: Box<H>,
}
```

`#[allow(dead_code)]` のままだと、将来本当に死蔵 (FFI 経由でも使われなくなる) になっても警告で気付けない。`#[expect(dead_code, reason = "FFI outputCallbackRefCon にポインタを渡すため Box で生存させる")]` に置換すると、使われ始めた瞬間に Rust が警告を出してくれる。

`#[expect]` は Rust 1.81+ で安定化済み。プロジェクトの `Cargo.toml` は `rust-version = "1.88"` であり、置換可能。

## 設計方針

それぞれ独立した小さい修正なので、1 ブランチ・1 PR でまとめて対応する。

## 完了条件

- `src/encoder.rs:1547-1551` の `mod tests` 直下コメントが必要最小限 (2 行目相当のみ) に縮約されている
- `src/encoder.rs:343-346` の `#[allow(dead_code)]` が `#[expect(dead_code, reason = "...")]` に置換されている
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` が通る

## 解決方法

1. `src/encoder.rs:1547-1551` のコメントを 1 行に縮約:

   ```rust
   //! `next_input_pts` など private フィールド検証専用のテスト。通常系は tests/test_encoder.rs。
   ```

2. `src/encoder.rs:344` の `#[allow(dead_code)]` を以下に置換:

   ```rust
   #[expect(dead_code, reason = "FFI outputCallbackRefCon にポインタを渡すため Box で生存させる")]
   ```
