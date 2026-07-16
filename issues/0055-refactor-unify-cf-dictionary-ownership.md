# `cf_dictionary` の返り値を `cf_array` と同じ `CfPtr` ガード付きに統一する

- Priority: Low
- Created: 2026-07-16
- Completed:
- Model: Fable 5
- Branch: feature/refactor-unify-cf-dictionary-ownership
- Polished: {YYYY-MM-DD}

## 目的

`src/types.rs` の CF オブジェクト生成ヘルパー間で、所有権の扱いが不統一になっている。

- `cf_array` (`src/types.rs:105-120`) は `CfPtr<c_void>` (Drop で `CFRelease` するガード) を返す
- `cf_number_i32` / `cf_number_i64` / `cf_number_f64` (`src/types.rs:122-168`) も `CfPtr<c_void>` を返す
- `cf_dictionary` (`src/types.rs:78-99`) だけが生の `CFDictionaryRef` を返し、呼び出し側が毎回手動で `CfPtr` に包んでいる

呼び出し側の手動ラップは以下の 5 箇所に散在しており、いずれも「生成 → 直後に手動ガード」という同じ定型を繰り返している。将来の呼び出し追加時にガードを書き忘れるとリークする構造のため、生成ヘルパー側でガードを返す形に統一する。

- `src/encoder.rs:449-450` (`reconfigure`): `cf_dictionary` の直後に `CfPtr(properties_dict.cast::<c_void>())`
- `src/encoder.rs:552-553` (`create_compression_session`): 同上
- `src/encoder.rs:979` / `src/encoder.rs:1069` (`encode` / `encode_pixel_buffer` の `frame_properties`): `Option<CfPtr<c_void>>` による条件付きガード
- `src/decoder.rs:319-320` (`create_decompression_session`): `cf_dictionary` の直後に `CfPtr(dest_attrs.cast::<c_void>())`

## 優先度根拠

- 現状の 5 箇所はすべて正しくガードされておりリークは無い (機能面の問題ではない)
- 生成ヘルパーの返り値型の不統一は、新しい呼び出し箇所を書くときのガード書き忘れ (リーク) を誘発する構造的な問題
- encoder / decoder 両方に波及する横断的な変更のため、大きな機能変更と重ねず単独で対応するのが安全
- 緊急性は無いため Low

## 現状

`cf_dictionary` の定義 (`src/types.rs:78-99`):

```rust
pub(crate) fn cf_dictionary(
    kvs: &[(sys::CFStringRef, *const c_void)],
) -> Result<sys::CFDictionaryRef, Error> {
    ...
    Ok(ptr)
}
```

呼び出し側の定型 (`src/encoder.rs:449-450` の例):

```rust
let properties_dict = cf_dictionary(&properties)?;
let _properties_dict_guard = CfPtr(properties_dict.cast::<c_void>());
let status = sys::VTSessionSetProperties(self.session.cast(), properties_dict);
```

## 設計方針

`cf_dictionary` の返り値を `CfPtr<c_void>` に変更し、`cf_array` / `cf_number_*` と揃える。FFI 関数へ渡す箇所では `guard.0` から生ポインタを取り出す (必要に応じて `.cast()` する)。

`encode` / `encode_pixel_buffer` の `frame_properties` は「`force_key_frame` 時のみ辞書を作り、それ以外は NULL を渡す」分岐があるため、`Option<CfPtr<c_void>>` を組み立ててから `as_ref().map_or(std::ptr::null(), |g| g.0.cast())` のような形で生ポインタを導出する。

## 完了条件

- `cf_dictionary` が `CfPtr<c_void>` を返し、全呼び出し箇所 (encoder 4 箇所 + decoder 1 箇所) から手動の `CfPtr(...)` ラップが消えている
- `cargo test --all` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo fmt --all -- --check` が通る
- 挙動変更が無いこと (リファクタリングのみ)
