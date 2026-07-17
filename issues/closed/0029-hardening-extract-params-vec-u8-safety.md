# パラメータセット抽出の `vec_u8_from_raw_parts_safe` がポインタ・長さの組み合わせに依存する

Created: 2026-04-01
Model: Composer 2 Fast

## なぜこの対応が必要か

エンコード出力のビットストリームは `CMBlockBufferCopyDataBytes` に変更済み（issue 0021）だが、**H.264 / H.265 のパラメータ抽出**（SPS / PPS / VPS）では **`CMVideoFormatDescriptionGetH264ParameterSetAtIndex` 等で得たポインタとサイズ**を `vec_u8_from_raw_parts_safe` に渡している。**ポインタが NULL でサイズが正**や、**サイズが実バッファより大きい**等、API 契約外の組み合わせでは **`to_vec()` 時の範囲外読み**の余地が理論上残る（`CMBlockBuffer` の `data_pointer_len` とは無関係）。

## 現状

- **場所**: `src/lib.rs` の `extract_h264_params` / `extract_h265_params` および `vec_u8_from_raw_parts_safe`

## 問題（リスク）

- `vec_u8_from_raw_parts_safe` は **NULL と len==0** 等は弾くが、**`len` が実バッファ長を超える**ケースは **API 契約に依存**する。
- **前任レビューで甘かった点**: 「追加の上限で保険」と書いたが、**上限値の根拠**（仕様・実測・Apple の最大値）がないと **恣意的な定数**になり、**偽陽性でパラメータ抽出失敗**を招く。

## 望ましい対応の方向（案）

- **根拠付き**の上限（例: H.264 の SPS の現実的な最大、または Apple の返却仕様）を issue またはコメントに引用してから `len` をクランプまたは拒否。
- 拒否とクランプの **ポリシー**を分ける（クランプはビットストリーム破壊のリスク）。

## 解決の完了条件（厳格）

- 上限を入れる場合、**数値の根拠**がレビューで追試できること（仕様・Issue・ベンチマークのいずれか）。
- 根拠なしの **マジックナンバー**のみの PR は **却下**してよい。

## 厳格レビュー（第 2 パス）

1. **ソース照合**: `extract_h264_params` / `extract_h265_params` の `vec_u8_from_raw_parts_safe` を確認。
2. **不足していた観点**: **上限導入の副作用**（偽陽性）。
3. **0021 との関係**: 圧縮データ経路は **別途対済**。混同禁止。
4. **誤解しやすい点**: `vec_u8_from_raw_parts_safe` が **すべての UB を防ぐ**わけではない（`len` がデカすぎると `to_vec()` が読み取り過ぎ）。
5. **前回からの差分**: 完了条件に **根拠引用必須**。

## 解決方法

Completed: 2026-04-01（2026-04-01 追記: 数値根拠を ISO/IEC 14496-15 に固定）

- `vec_u8_from_raw_parts_safe` で `parameterSetSize` が **`u16::MAX`（65535）バイト**を超える場合は拒否する。
- **上限の根拠**: ISO/IEC 14496-15 の `AVCDecoderConfigurationRecord`（`sequenceParameterSetLength` / `pictureParameterSetLength`）および `HEVCDecoderConfigurationRecord` 側の NAL 長フィールドが **`unsigned int(16)`** であるため、コンテナ上で表現可能な 1 パラメータセットあたりの最大長は **65535 バイト**である。これは issue の完了条件「仕様に基づく数値根拠の追試可能性」を満たす。
- 実装は `src/lib.rs` の `MAX_PARAMETER_SET_COPY_BYTES` コメントに上記を日本語で記載する。
