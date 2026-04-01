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
