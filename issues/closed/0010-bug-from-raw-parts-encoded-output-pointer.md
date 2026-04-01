# エンコード出力の `from_raw_parts` が NULL ポインタや長さ 0 の組み合わせで未定義動作になり得る

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`process_encoded_output` で、圧縮データを `std::slice::from_raw_parts(data_pointer as *const u8, data_pointer_len)` として `Vec` にコピーしている。Rust では、`slice::from_raw_parts` は **ポインタと長さの組み合わせ**に厳格な前提がある。特に **長さ 0 のときでもポインタは非 NULL かつ適切にアライメントされた有効なアドレス**が求められることが多く、`(NULL, 0)` は **未定義動作**になり得る。また **NULL かつ長さが正**も未定義動作である。堅牢性のため、前提を満たすか明示的に分岐する必要がある。

## 現状

- **場所**: `src/lib.rs` の `Encoder::process_encoded_output`
- `CMBlockBufferGetDataPointer` の結果 `data_pointer` および `data_pointer_len` をそのまま `from_raw_parts` に渡している。

## 問題

- `data_pointer_len == 0` かつ `data_pointer == NULL` のとき、`from_raw_parts` が **UB**。
- `data_pointer_len > 0` かつ `data_pointer == NULL` のときも **UB**。

## 望ましい対応の方向（案）

- `data_pointer_len == 0` のときは `Vec::new()` などで扱い、`from_raw_parts` を呼ばない。
- 長さが正のときは `data_pointer` が NULL でないことを検証してから `from_raw_parts` を呼ぶ。
- Apple の API が常に有効なポインタを返すかはドキュメントで確認し、コメントに根拠を残す。

## 解決方法

`vec_u8_from_raw_parts_safe` を追加し、`process_encoded_output` では `CMBlockBufferGetDataPointer` の結果をこのヘルパー経由で `Vec<u8>` にコピーするようにした。長さ 0 のときは `Vec::new()` とし、長さが正でポインタが NULL のときはログして処理を打ち切る。

## 解決方法（再対応）

`CMBlockBufferGetDataLength` と `data_pointer_len` を比較し、`data_pointer_len` がブロック長を超える場合はログして打ち切る。

## 解決方法（レビュー追補・0021）

上記の照合では **`data_pointer_len` が `block_len` より小さい**非連続バッファを検出できず、連続領域の長さだけコピーして **フレームを切り詰ける**問題が残っていた。`0021-bug-cmblockbuffer-getdatapointer-partial-length.md` のとおり、`CMBlockBufferCopyDataBytes` で **`CMBlockBufferGetDataLength` 分**をコピーする方式に変更した。
