# `Decoder::decode` 内でビットストリームを解析し、SPS/PPS/シーケンスヘッダ変更を自動検出して `update_format` を呼ぶ

Created: 2026-05-10
Model: Opus 4.7 (claude-opus-4-7[1m])

## 概要

現在の `Decoder` は SPS/PPS や解像度の変更を検出するために、利用側が明示的に `Decoder::update_format()` を呼ぶ必要がある。
これを廃止し、`Decoder::decode()` 内でビットストリームを解析して **自動的にフォーマット変更を検出** し、内部で `update_format` 相当の処理を行う。
最終的に公開 API としての `update_format` は削除する。

## 背景

他ベンダー API のラッパー (`nvcodec-rs`, `vpl-rs`, `amf-rs`) はいずれも、デコーダーに対して利用側が `decode()` を呼ぶだけでよく、解像度・フォーマット変更の検出は **ライブラリ内部で自動的に処理** される。

- `nvcodec-rs`: CUVID パーサーの `pfnSequenceCallback` で内部再構築
- `vpl-rs`: 初回 `decode()` 内で `MFXVideoDECODE_DecodeHeader` + `Init` を自動実行
- `amf-rs`: AMF コンポーネントが `SubmitInput` 内で自動処理

VideoToolbox は `VTDecompressionSession` がバイトストリームパーサーを持たない低レベル API のため、現状の `update_format` を利用側に強要する設計になっている。
本ライブラリ内部でパースを実装すれば、他のベンダーラッパーと一貫した「`decode()` だけで済む」API に揃えられる。

## 対応方針

### コーデック別のフォーマット変化検出

| コーデック | 検出対象 NAL / OBU | 解析難易度 |
|---|---|---|
| H.264 | NAL type 7 (SPS), 8 (PPS) を AVCC ストリームから走査 | 容易 |
| H.265 | NAL type 32 (VPS), 33 (SPS), 34 (PPS) | 容易 |
| VP9 | uncompressed header の `width_minus_1` / `height_minus_1` を bit パース | 中 |
| AV1 | OBU sequence header をパース | 高 |

最初の段階では H.264 / H.265 のみ対応し、VP9 / AV1 は段階的に拡張する。

### 検出ロジック

1. `Decoder::decode(data)` の入力 (AVCC) を走査する
2. SPS/PPS (H.264/H.265 では VPS も) を見つけたら、前回保持している値とバイト列で比較する
3. 変化があれば内部で `update_format` 相当の処理を実行する
   - 新しい `CMVideoFormatDescription` を作成
   - `VTDecompressionSessionCanAcceptFormatDescription` で受け入れ可否を判定
   - 受け入れ可能なら description のみ差し替え、不可能ならセッションを再作成
4. パースに失敗した場合、もしくは SPS/PPS が含まれていない場合は何もせず既存セッションでデコードを続行する

### 公開 API への影響

- `Decoder::update_format()` を **削除する** (破壊的変更)
- `DecoderConfig::codec` には初期化時の SPS/PPS / 解像度を引き続き渡す。これは初期セッション作成のためにそのまま必要
- `Decoder::decode()` のシグネチャは変更しない (呼び出し側の `decode(data, user_data)` はそのまま)
- `DecoderCodec` 自体は内部で再利用するため公開のまま残すか、内部用に切り出すかは実装時に判断

### 失敗・例外時の挙動

- ビットストリーム解析に失敗 → ログ出力のみ、既存セッションでデコード続行 (フェイルオープン)
- 検出した SPS/PPS で `update_format` 内部処理が失敗 → エラーを `decode()` の戻り値で返す
- パース対象でない (例: SPS/PPS が含まれない P フレーム) → スルー

## 変更対象ファイル

- `src/decoder.rs`: ビットストリーム解析ロジック、`Decoder::decode` 内の自動検出処理、`update_format` の削除
- `src/lib.rs`: `update_format` の re-export 削除 (該当があれば)
- `tests/test_decoder.rs`: 自動検出のテスト (SPS が途中で変わるストリーム / 変わらないストリーム / 不正なバイト列)
- `pbt/tests/prop_decoder.rs`: 任意の AVCC バイト列を入力としてパニックしないことを検証
- `README.md`: 「動的設定変更」セクションのデコーダー側説明を「`decode()` 呼び出しだけで自動的にフォーマット変更を追従する」に書き換え。`update_format` のサンプル削除
- `CHANGES.md`: `[CHANGE] Decoder::update_format を削除し、Decoder::decode で SPS/PPS 変更を自動検出する` を追記

## 破壊的変更

- `Decoder::update_format` を削除する
- 利用側コードからは `decoder.update_format(...)` の呼び出しを削除するだけで移行できる (引数だった `DecoderCodec` は `decode()` に渡すバイト列に含まれているため不要)

## 段階的実装の選択肢

1. (このイシュー) H.264 / H.265 のパースと自動検出を実装し、`update_format` を削除する
2. (フォローアップ) VP9 のパース対応
3. (フォローアップ) AV1 のパース対応

VP9 / AV1 が未対応の段階では、該当コーデックでフォーマット変更が起きると検出できずデコード結果が不正になる可能性がある。
段階 1 で `update_format` を削除した後 VP9 / AV1 ストリームで途中変更が起きないことが保証されない場合は、段階 2 / 3 を完了させてからリリースする方針も検討する。

## 補足

`AnnexB` 形式の入力サポートはこの issue のスコープ外。本ライブラリは現状 AVCC を前提とする。
将来、AnnexB のままデコードしたい要件が出た場合は別 issue とする。
