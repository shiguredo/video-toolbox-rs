# `extern "C"` コールバック内の panic が未定義動作になる

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-extern-c-callback-panic
- Polished: {YYYY-MM-DD}

## 目的

エンコーダー・デコーダーの FFI コールバック（`output_callback_h264` / `output_callback_h265` / `output_callback`）は `extern "C"` 関数として定義されている。ユーザー実装の `on_encoded` / `on_decoded` が panic した場合、unwind が `extern "C"` の ABI 境界を越え未定義動作となる。

## 現状

`src/encoder.rs` の `output_callback_h264` / `output_callback_h265`、`src/decoder.rs` の `output_callback` は `unsafe extern "C" fn` で定義されている。`invoke_callback` 経由でユーザーのハンドラを呼び出すが、panic の保護がない。

Rust Reference は "Unwinding through extern \"C\" functions is undefined behavior" と定めている。

## 設計方針

以下のいずれかを採用する（プロジェクトレベルの判断が必要）:

1. `extern "C-unwind"` に変更する（Rust 1.71 で安定化）
2. CODEBASE.md に例外を記載した上で `catch_unwind` で保護する（shiguredo-rust 規約の `catch_unwind` 禁止との衝突を解決する必要がある）

## 完了条件

ユーザーハンドラが panic しても未定義動作が発生しないこと。
