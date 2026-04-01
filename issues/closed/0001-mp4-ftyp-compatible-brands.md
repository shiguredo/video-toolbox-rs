# mp4-rs の Mp4FileMuxer で ftyp の compatible_brands をカスタマイズできない

## 概要

`shiguredo_mp4::mux::Mp4FileMuxer` は `build_initial_boxes()` 内で ftyp ボックスの compatible_brands を `[isom, iso2, mp41, avc1, av01]` にハードコードしている。
そのため H.265 エンコード時に適切な brands (`hev1`) を設定できない。

## 解決

shiguredo_mp4 2026.2.0-canary.3 で対応済み。

`Mp4FileMuxer::finalize()` 時に `build_final_ftyp_box()` が呼ばれ、実際に使用した `SampleEntry` に応じて compatible_brands が自動設定されるようになった（avc1, hev1, hvc1, av01）。

example 側のハック（ftyp 上書き処理）を削除し、shiguredo_mp4 の正式な機能に置き換えた。
