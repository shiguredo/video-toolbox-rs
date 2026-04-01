# エンコード出力で `CMBlockBufferGetDataLength` に基づく `Vec` 確保に上限がない

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`process_encoded_output` は `CMBlockBufferGetDataLength` の値で `vec![0u8; block_len]` を確保する。**異常に大きい長さ**（破損データ・実装バグ・悪意ある入力に近い経路）では **OOM** や **サービス拒否相当**になり得る。他の箇所ではパラメータセット長に **ISO 14496-15 に沿った上限**を設けている一方、ここには **防御的上限がない**。

## 現状

- **場所**: `src/lib.rs` の `Encoder::process_encoded_output`（`CMBlockBufferCopyDataBytes` 前の `Vec` 確保）

## 望ましい対応の方向（案）

- 妥当な **最大フレームサイズ**（解像度・ビットレート・コーデックから導くか、定数上限）を検討し、超過時は **エラー扱い・ログ・フレーム破棄**のいずれかで明確に失敗させる（無暗にクランプしないこと。ビットストリームを壊す可能性）。

## 解決の完了条件

- 上限方針が決まり、**コードと短いコメント**で根拠が追試できること。
