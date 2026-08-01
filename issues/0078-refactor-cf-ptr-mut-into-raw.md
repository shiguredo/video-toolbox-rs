# `CfPtrMut` に所有権を取り出す `into_raw` を追加する

- Created: 2026-08-01
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-cf-ptr-mut-into-raw
- Polished: {YYYY-MM-DD}

## 目的

`src/encoder.rs` の `create_compression_session` の成功パスで `std::mem::forget` を直接呼んで `CfPtrMut` の所有権を放棄しているが、`forget` の書き忘れはコンパイルエラーにならず、`CfPtrMut` の `Drop` による `CFRelease` と `Encoder::drop` の `VTCompressionSessionInvalidate` + `CFRelease` が重なって二重解放・use-after-free になる。所有権の移転を構造的に安全にするため、`CfPtrMut` に raw ポインタを取り出す `into_raw` メソッドを追加する。

## 現状

- `src/types.rs` の `CfPtrMut` には所有権を取り出すメソッドがなく、所有権を放棄するには `std::mem::forget` を直接呼ぶしかない
- `src/encoder.rs` の `create_compression_session` の成功パスで `std::mem::forget(session_guard)` を使っている（このリポジトリで唯一の使用箇所）
- `forget` の書き忘れは静的解析で検出できないため、コメントとコードレビューでしか防げない

## 設計方針

`CfPtrMut` に `into_raw(self) -> *mut T` メソッドを追加する。内部で `std::mem::forget(self)` を行ってから raw ポインタを返すことで、呼び出し側は `let session = session_guard.into_raw();` のように所有権の移転を明示的に書ける。`CfPtrMut` は `pub(crate)` のため公開 API への影響はない。

## 完了条件

- `src/types.rs` の `CfPtrMut` に `into_raw` メソッドを追加する
- `src/encoder.rs` の `create_compression_session` の成功パスを `into_raw` を使う形に置き換える
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る
