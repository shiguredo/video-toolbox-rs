# `codec_info` で Core Foundation オブジェクトの型を検証せずにキャストしている

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`src/codec_info.rs` は `VTCopyVideoEncoderList` / `VTCopySupportedPropertyDictionaryForEncoder` 等から得た `CFTypeRef` 相当の値を、**`CFGetTypeID` なしで** `CFDictionaryRef` / `CFArrayRef` / `CFStringRef` / `CFNumber` として扱っている。要素が **想定型でない**場合、Core Foundation の前提違反となり、**実装依存のクラッシュや不正読み取り**になりうる。NULL チェックと **型は別**である。

本件は **同一ファイル・同一方針（型 ID 照合）**で直せるため、旧 issue **0024 / 0025 / 0026** を **1 本に統合**した。

## 現状（箇所別）

### A. `probe_encoding`：エンコーダ一覧の配列要素

- `CFArrayGetValueAtIndex` の戻りを **`CFDictionaryRef` にキャスト**し、`CFDictionaryGetValue` / `get_cf_bool(entry, ...)` に渡している。
- **`entry` が辞書型か**は未検証（NULL のみ）。

### B. `probe_encoding`：`kVTVideoEncoderList_CodecType`

- `CFDictionaryGetValue` の戻りに対し **`CFNumberGetValue(..., kCFNumberSInt32Type, ...)`** を呼んでいる。
- **`CFNumber` 型か**は未検証（NULL のみ）。`ok` だけでは **型の取り違え**に弱い。

### C. `query_encoding_profiles` / `match_profiles`：プロファイル経路

- `kVTCompressionPropertyKey_ProfileLevel` の値を **`CFDictionaryRef` にキャスト**してからネストした `CFDictionaryGetValue` を呼んでいる。
- `value_list` を **`CFArrayRef` にキャスト**し、`CFArrayGetCount` を呼んでいる。
- `match_profiles` で配列要素を **`CFStringRef` にキャスト**している。
- **辞書・配列・文字列**いずれも **`CFGetTypeID` は未使用**（NULL チェックのみ）。

## 問題

- 上記 A〜C いずれも、キャスト直前に **期待する `CFTypeID` との一致**がない。
- **前任レビューで甘かった点**: 0024〜0026 を分けたことで **同一 PR で直すべき経路**が分散していた。

## 望ましい対応の方向（案）

- **A**: `entry` を辞書として扱う直前に `CFGetTypeID(entry as CFTypeRef) == CFDictionaryGetTypeID()`。失敗時は `continue`。
- **B**: `CFNumberGetValue` の直前に `CFGetTypeID(...) == CFNumberGetTypeID()`。（任意）格納型の追加確認。
- **C**: `profile_entry`（辞書）、`value_list`（配列）、`match_profiles` の各要素（文字列）を、**それぞれ**キャスト直前に型確認。

## 解決の完了条件（厳格）

- `CFDictionaryGetValue` / `get_cf_bool` / `CFNumberGetValue` / `CFArrayGetCount` / `CFArrayGetValueAtIndex` に至る **各経路**で、**直前の型前提がコードで保証**されていること。
- 「`ok` を見れば足りる」だけの **B 向け PR** は **却下**してよいレベルで、**型前提**を満たすこと。

## 厳格レビュー（第 2 パス・統合時）

1. **ソース照合**: `codec_info.rs` の `probe_encoding` / `query_encoding_profiles` / `match_profiles` を一括で追うこと。
2. **A と B**: 同ループ内でも **`entry` が辞書でも `codec_type_value` は別キー**の値として検証が要る。
3. **C**: 「ProfileLevel さえ辞書ならよい」ではなく、**配列・文字列の下流**も含める。
4. **誤解しやすい点**: 「Apple が返すから安全」は **環境差に弱い**。
5. **統合理由**: 修正方針が **`CFGetTypeID` ガード**で一本化できるため。

## 解決方法

Completed: 2026-04-01

- `probe_encoding` で配列要素・`kVTVideoEncoderList_CodecType` の値を辞書・`CFNumber` として扱う前に `CFGetTypeID` で検証する。
- `query_encoding_profiles` で `props`・`profile_entry`・`value_list` をそれぞれ辞書・配列として扱う前に検証する。
- `match_profiles` で配列要素を文字列として扱う前に `CFStringGetTypeID` と照合する。
