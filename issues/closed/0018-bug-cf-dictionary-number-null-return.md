# `CFDictionaryCreate` / `CFNumberCreate` の戻り NULL を扱っていない

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`cf_dictionary` および `cf_number_i32` / `cf_number_i64` / `cf_number_f64` は、`CFDictionaryCreate` / `CFNumberCreate` の戻り値をそのまま `CfPtr` / `CfPtrMut` に格納している。メモリ不足等で **NULL が返る**場合、`CfPtr` の `Drop` で `CFRelease(NULL)` になる。多くの環境では **NULL への `CFRelease` は無害**とされるが、**ドキュメント上「常に NULL 安全」と明記されているわけではない**。NULL のオブジェクトを後続 API に渡す経路が増えると、**別種の未定義動作**に繋がり得る。

## 現状

- **場所**: `src/lib.rs` の `cf_dictionary`、`cf_number_i32`、`cf_number_i64`、`cf_number_f64`
- 戻り値の NULL チェックはない。

## 問題

- 極端なメモリ不足時に NULL が返った場合の挙動が未定義に近い。

## 望ましい対応の方向（案）

- `ptr.is_null()` のときは `Error` またはパニック（方針はプロジェクトで決定）にする。
- Apple の仕様を確認し、NULL の場合の推奨をコメントに残す。

## 解決方法

`cf_dictionary` / `cf_number_i32` / `cf_number_i64` / `cf_number_f64` を `Result` にし、戻りが NULL のときは `Error::LimitExceeded` を返す。呼び出し側（`add_common_properties`、セッション作成、エンコードの辞書生成等）で `?` を使う。

## 解決方法（再対応）

戻りが NULL のときは `Error::CfObjectCreationFailed` を返す（関数名を保持）。PTS オーバーフローやプレーン算術は引き続き `LimitExceeded` とする。
