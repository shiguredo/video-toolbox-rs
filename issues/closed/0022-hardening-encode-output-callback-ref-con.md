# エンコード出力コールバックの `output_callback_ref_con` と `unsafe` 契約を実装側で明示する

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`output_callback_ref_con` を `*mut std::sync::mpsc::Sender<EncodedFrame>` にキャストし、`let tx = &*(output_callback_ref_con as *mut Sender<EncodedFrame>)` の形で **`&Sender` を構成**してから `send` している。ここは **Rust の `unsafe` 契約**（ポインタが **`Sender` として有効・アライメント・エイリアス規則を満たす**こと）に直結する。

本件は **`Encoder` の内部実装**であり、利用者が `VTCompressionSessionCreate` に `outputCallbackRefCon` を渡すわけではない。**公開 API のドキュメント**に `Sender` の寿命や `Invalidate` の保証まで書くことは、**内部都合を利用者契約に押し出しすぎる**。論点は **実装ファイル内のコメント**と **`unsafe` 境界の整理**に置く。

## 現状

- **登録**: `src/lib.rs` の `Encoder::create_compression_session` 内で、`VTCompressionSessionCreate` の `outputCallbackRefCon` に **`&Sender` のアドレス**（`Box<Sender>` 経由の `&Sender` を `*mut c_void` にキャスト）を渡している。
- **使用**: `process_encoded_output` 末尾で同型のポインタから `&Sender` を復元して `send` している。
- **Drop**: `Encoder::drop` で `VTCompressionSessionInvalidate` の後にセッション解放、`Sender` は `Encoder` のフィールドとして保持。

## 問題（リスク）

- `VTCompressionSession` の登録とコールバック内の **`&*` 復元**が、**同一の unsafe 前提**としてコード上対応付いていないと、メンテ時に壊しやすい。
- **前任レビューで甘かった点**: 完了条件に **公開ドキュメント**を求めすぎた（本レビューで修正）。

## 望ましい対応の方向（案）

- `create_compression_session` と `process_encoded_output` の **近接する位置**に、**日本語コメント**で次を対応付ける: 登録時に渡すポインタと、コールバックで復元するポインタが **同一の `Sender` を指す**こと；そのポインタが **`Encoder` の生存期間中**有効であること。
- （任意）キャストを単一点に集約する newtype や `unsafe fn` への集約。
- **公開 API の `Encoder` の doc コメント**にまで **必須としない**（利用者向けに書くなら「内部で mpsc を使う」程度にとどめる）。

## 解決の完了条件

- 上記 **unsafe 契約**が **`src/lib.rs` 内のコメント**（または `unsafe` ブロック直前の短い説明）で **追試可能**であること。
- **公開 API ドキュメントへの Invalidate / Sender 寿命の根拠引用**を **完了条件に含めない**。

## レビュー指摘への反映（2026-04-01）

- 旧「公開 API で Sender の寿命を書く」「Invalidate の保証を公開ドキュメントに」の **要求を撤回**し、**内部実装の明示**に寄せた。

## レビュー確認（指摘とソース照合）

| 指摘 | 確認結果 |
|------|----------|
| 論点は内部の `Sender` ポインタ往復であり、公開 API まで責務を広げすぎない | **妥当**。`create_compression_session` / `process_encoded_output` は **クレート内部**の `unsafe` 経路。利用者は `Encoder::new` 等のみ。 |
| 解決は内部コメント・unsafe 境界の整理が主 | **妥当**。完了条件を **`src/lib.rs` 内のコメント**に限定済み。 |
| `VTCompressionSessionCreate` と `output_callback_ref_con` | `create_compression_session`（約 385–398 行付近）と `process_encoded_output`（約 1048 行付近）で対応。 |

## 解決方法

Completed: 2026-04-01

- `create_compression_session` 内の `VTCompressionSessionCreate` 直前と、`process_encoded_output` 内の `Sender` 復元直前に、日本語コメントで `outputCallbackRefCon` と `Box` 内 `Sender` の対応および生存期間を明示した。
