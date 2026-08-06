# `cf_dictionary` の返り値を `cf_array` と同じ `CfPtr` ガード付きに統一する

- Priority: Low
- Created: 2026-07-16
- Updated: 2026-07-21
- Completed: 2026-08-06
- Model: Fable 5
- Branch: feature/refactor-unify-cf-dictionary-ownership
- Polished: 2026-07-31

## 目的

`src/types.rs` の CF オブジェクト生成ヘルパー間で、所有権の扱いが不統一になっている。

- `cf_array` は `CfPtr<c_void>` (Drop で `CFRelease` するガード) を返す
- `cf_number_i32` / `cf_number_i64` / `cf_number_f64` も `CfPtr<c_void>` を返す
- `cf_dictionary` だけが生の `CFDictionaryRef` を返し、呼び出し側が毎回手動で `CfPtr` に包んでいる

呼び出し側の手動ラップは以下の 5 箇所に散在しており、いずれも「生成 → 直後に手動ガード」という同じ定型を繰り返している。将来の呼び出し追加時にガードを書き忘れるとリークする構造のため、生成ヘルパー側でガードを返す形に統一する。

- `src/encoder.rs` の `Encoder::reconfigure`: `cf_dictionary` の直後に `CfPtr(properties_dict.cast::<c_void>())`
- `src/encoder.rs` の `create_compression_session`: 同上
- `src/encoder.rs` の `encode` / `encode_pixel_buffer` の `frame_properties`: `Option<CfPtr<c_void>>` による条件付きガード
- `src/decoder.rs` の `create_decompression_session`: `cf_dictionary` の直後に `CfPtr(dest_attrs.cast::<c_void>())`

## 優先度根拠

- 現状の 5 箇所はすべて正しくガードされておりリークは無い (機能面の問題ではない)
- 生成ヘルパーの返り値型の不統一は、新しい呼び出し箇所を書くときのガード書き忘れ (リーク) を誘発する構造的な問題
- encoder / decoder 両方に波及する横断的な変更のため、大きな機能変更と重ねず単独で対応するのが安全
- 緊急性は無いため Low

## 現状

`cf_dictionary` の定義 (`src/types.rs`):

```rust
pub(crate) fn cf_dictionary(
    kvs: &[(sys::CFStringRef, *const c_void)],
) -> Result<sys::CFDictionaryRef, Error> {
    ...
    Ok(ptr)
}
```

呼び出し側の定型 (`src/encoder.rs` の `Encoder::reconfigure` の例):

```rust
let properties_dict = cf_dictionary(&properties)?;
let _properties_dict_guard = CfPtr(properties_dict.cast::<c_void>());
let status = sys::VTSessionSetProperties(self.session.cast(), properties_dict);
```

## 設計方針

`cf_dictionary` の返り値を `CfPtr<c_void>` に変更し、`cf_array` / `cf_number_*` と揃える。FFI 関数へ渡す箇所では `guard.0` から生ポインタを取り出し、`.cast()` する（FFI 引数はすべて `CFDictionaryRef` のため、全箇所で `.cast()` が必要になる）。あわせて、返り値がガード済みで drop 時に `CFRelease` される旨の所有権 doc コメントを `cf_dictionary` に付ける。

`encode` / `encode_pixel_buffer` の `frame_properties` は「`force_key_frame` 時のみ辞書を作り、それ以外は NULL を渡す」分岐があるため、`Option<CfPtr<c_void>>` を組み立ててから生ポインタを別変数に導出する。ガード（`Option<CfPtr>`）は FFI 呼び出しまで生存させる必要があるため、シャドーイングで同名変数に潰すとガードが早期 drop され use-after-free になる点に注意する。例:

```rust
let frame_properties = if options.force_key_frame {
    Some(cf_dictionary(&[...])?)
} else {
    None
};
let frame_properties_guard = frame_properties;
let frame_properties_ptr =
    frame_properties_guard.as_ref().map_or(std::ptr::null(), |g| g.0.cast());
```

## 関連 issue

- issue 0053（`src/encoder.rs` のモジュール分割）: 本 issue の変更対象のうち encoder 側の 4 箇所は分割後は `mod.rs`（`reconfigure`）/ `session.rs`（`create_compression_session`）/ `pixel_buffer.rs`（`encode` / `encode_pixel_buffer`）に分散する。どちらを先に実施しても成立する（先に 0053 を実施する場合は、本 issue の変更対象は分割後のモジュール内の関数になる）
- issue 0073（`encode` / `encode_pixel_buffer` の重複解消）: `frame_properties` の構築と同じ行域を対象とする。どちらを先に実施しても成立する（先に 0073 を実施する場合は、本 issue の変更対象のうち `frame_properties` 構築は抽出後の共通メソッド内の 1 箇所に減る）

## 完了条件

- `cf_dictionary` が `CfPtr<c_void>` を返し、全呼び出し箇所 (encoder 4 箇所 + decoder 1 箇所) から手動の `CfPtr(...)` ラップが消えている
- `CHANGES.md` の `## develop` に `[UPDATE]` としてリファクタリングのエントリを追記する（公開 API の変更を伴わないため `### misc` サブセクション）
- `cargo test --workspace` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo fmt --all -- --check` が通る
- 挙動変更が無いこと (リファクタリングのみ)

## 解決方法

`src/types.rs` の `cf_dictionary` の返り値を `sys::CFDictionaryRef` から `CfPtr<c_void>` に変更し、`cf_array` / `cf_number_*` と同じく drop 時に `CFRelease` されるガードを返すようにした。あわせて所有権 doc コメント (要素 retain・ガードの生存契約) を追加した。

呼び出し側 5 箇所の手動ラップを削除した:

- `src/encoder.rs` の `Encoder::reconfigure`: `properties_dict.0.cast()` を直接 FFI に渡す
- `src/encoder/session.rs` の `create_compression_session`: 同上
- `src/encoder/pixel_buffer.rs` の `encode` / `encode_pixel_buffer`: `Option<CfPtr<c_void>>` を組み立て、`map_or` で生ポインタを導出して FFI に渡す (ガードは FFI 呼び出しまで生存)
- `src/decoder.rs` の `create_decompression_session`: `dest_attrs.0.cast()` を直接 FFI に渡す

挙動変更はなく、全パスでガードの生存期間が FFI 呼び出しを跨ぐことを確認した。`CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記した。`cargo test --workspace` は全 46 テストがパスする。
