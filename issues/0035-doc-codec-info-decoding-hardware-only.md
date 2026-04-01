# `supported_codecs` のデコード可否がハードウェア判定のみであることを API ドキュメントに明記する

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`codec_info::probe_decoding` は `VTIsHardwareDecodeSupported` の結果で **`supported` と `hardware_accelerated` を同一にしている**。ソフトウェアデコードのみ利用可能な環境では **`supported: false`** になり得るが、利用者が **「Video Toolbox でデコードできない」**と誤解する余地がある。

## 現状

- **場所**: `src/codec_info.rs` の `probe_decoding` と `DecodingInfo` の公開意味

## 望ましい対応の方向（案）

- `supported_codecs` または `DecodingInfo` の **rustdoc** に、**ハードウェアデコード可否を返している**こと、および **ソフトウェアパスは反映されない**可能性があることを短く書く。

## 解決の完了条件

- docs.rs で読める **公開 API の説明**に上記が含まれること。

## レビュー（第 1 回）— ソース照合

- `probe_decoding` は `VTIsHardwareDecodeSupported(fourcc) != 0` を `supported` と `hardware_accelerated` の両方に使う。コメントにも「同じ値になる」とある。**命名と実装は一致**している。

## レビュー（第 2 回）— 誤解のパターン

- 利用者が `supported == false` を **「このクレートではデコード不可」**と読むのが主リスク。実際は **「ハードウェアデコードとしての可否」**に近い。文言はその語を入れると明確。

## レビュー（第 3 回）— `EncodingInfo` との対称性

- エンコード側は `VTCopyVideoEncoderList` ベースで別ロジック。**デコードだけ**注意書きが必要。対称美を求めてエンコード側に同レベルの注釈を足すかは任意。

## レビュー（第 4 回）— 公開 surface

- `supported_codecs` 関数と `DecodingInfo` 構造体の **両方**に書くと検索に引っかかりやすい。片方だけだと docs.rs の導線で見落としうる。

## レビュー（第 5 回）— Apple API の安定性

- `VTIsHardwareDecodeSupported` の意味が将来変わる可能性は低いが、**「あくまで VT API の返却に基づく」**と書くと将来も安全。

## レビュー（第 6 回）— 0034 との関係

- CI で `test_supported_codecs` が H.264 を必須にしていることと、**本 issue の「ソフトウェアのみ」**は別次元。混同しないこと。

## レビュー（第 7 回）— サンプルコード

- README の `supported_codecs` 利用例があれば、**一行コメント**で補足するのが効果的（rustdoc だけに頼らない）。

## レビュー（第 8 回）— 翻訳・表記

- 英語の rustdoc がプロジェクト規約と整合するか（ログ・エラーは英語、コメントは日本語）。**公開ドキュメントは英語**が一般的。

## レビュー（第 9 回）— 完了の検証

- docs.rs ではなく `cargo doc --open` で **レンダリング**を確認すると見出し階層のミスに気づきやすい。

## レビュー（第 10 回）— 締めチェックリスト

- [ ] `DecodingInfo::supported` の意味を一文で定義したか。
- [ ] ソフトウェアデコードが対象外になりうる旨を書いたか。
- [ ] 公開 API のどこに書いたか（関数・型）を決めたか。
