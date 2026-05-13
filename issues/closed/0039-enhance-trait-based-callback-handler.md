# エンコーダー/デコーダーのコールバック受け口を FnMut から trait ベースのハンドラーに変更する

Created: 2026-05-11
Completed: 2026-05-12
Model: deepseek-v4-pro

## なぜこの対応が必要か

現在の `Decoder` と `Encoder` は、完了コールバックを `F: FnMut(Result<*, Error>) + Send + 'static` というジェネリック境界で受け取っている。この方式には以下の問題がある:

1. **拡張性の欠如**: 将来的にコールバックに追加のライフサイクル管理や状態を持たせたい場合（例: バックプレッシャー通知、統計収集等）、`FnMut` では都度クロージャを書く必要がある。trait 化すればハンドラにメソッドを追加することで後方互換を保ちつつ拡張できる
2. **API の一貫性**: コールバック以外の設定は `Config` 構造体で受けているのに、コールバックだけ生の `FnMut` であり API の一貫性に欠ける。trait + ラッパー構造体で渡すことで、コールバックも構造化された値として受け取れる

## 現状と変更対象ファイル

### 変更対象ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `src/decoder.rs` | trait / wrapper 定義、`new()` シグネチャ、内部型エイリアス、`invoke_callback` 等 |
| `src/encoder.rs` | trait / wrapper 定義、`new()` シグネチャ、内部型エイリアス、`invoke_callback` 等 |
| `src/lib.rs` | 新規公開型の `pub use` 追加 |
| `tests/test_decoder.rs` | 全 `Decoder::new(...)` 呼び出し（約 6 箇所）を wrapper 経由に変更 |
| `tests/test_encoder.rs` | 全 `Encoder::new(...)` 呼び出し（約 14 箇所）を wrapper 経由に変更 |
| `examples/raden_to_mp4.rs` | `Encoder::new(config, closure)` を `Encoder::new(config, FnEncodeHandler::new(closure))` に変更 |
| `CHANGES.md` | `[ADD]` と `[CHANGE]` の 2 エントリを追記 |

### デコーダーの現状 (`src/decoder.rs`)

```rust
// 60 行目
type DecodeCallback<T> = dyn FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static;
// 64 行目
type BoxDecodeCallback<T> = Box<DecodeCallback<T>>;
// 88 行目（Decoder 構造体フィールド）
callback: Box<BoxDecodeCallback<T>>,
// 93 行目
pub fn new<F>(config: DecoderConfig<'_>, on_decoded: F) -> Result<Self, Error>
where
    F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static,
```

### エンコーダーの現状 (`src/encoder.rs`)

```rust
// 134 行目
type EncodeCallback<T> = dyn FnMut(Result<EncodedFrame<T>, Error>) + Send + 'static;
// 138 行目
type BoxEncodedCallback<T> = Box<EncodeCallback<T>>;
// 168 行目（Encoder 構造体フィールド）
callback: Box<BoxEncodedCallback<T>>,
// 173 行目
pub fn new<F>(config: EncoderConfig, on_encoded: F) -> Result<Self, Error>
where
    F: FnMut(Result<EncodedFrame<T>, Error>) + Send + 'static,
```

### ダブルボックス化の理由（維持すべき制約）

現在の実装では `callback: Box<Box<dyn FnMut(...)>>` と **二重に Box されている**。理由は以下:

1. `dyn FnMut(...)` は fat pointer (data ポインタ + vtable ポインタの 2 ワード) であり、そのままでは FFI の `*mut c_void` (1 ワード) として渡せない
2. 外側の `Box` のヒープアドレスは `Decoder` / `Encoder` の move 後も不変であるため、FFI コールバックの `decompressionOutputRefCon` / `outputCallbackRefCon` に渡したポインタが構造体の生存期間中有効であり続ける

`dyn DecodeHandler<T>` も fat pointer であるため、**trait 化後も二重 Box 構造は維持する**。

## 望ましい対応の方向

### 1. trait 定義

`Decoder<T: Send + 'static>` / `Encoder<T: Send + 'static>` がスレッドセーフにコールバックを受け取るため、trait には `Send + 'static` supertrait が必要である。`'static` はコールバック格納期間が無期限であることを表し、`Send` は Video Toolbox の別スレッドから呼ばれるための要件である。

```rust
// src/decoder.rs の type DecodeCallback<T> の直前に追加
pub trait DecodeHandler<T>: Send + 'static {
    fn on_decoded(&mut self, result: Result<DecodedFrame<T>, Error>);
}

pub struct FnDecodeHandler<F>(F);

impl<F> FnDecodeHandler<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<T, F> DecodeHandler<T> for FnDecodeHandler<F>
where
    F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static,
{
    fn on_decoded(&mut self, result: Result<DecodedFrame<T>, Error>) {
        (self.0)(result);
    }
}
```

```rust
// src/encoder.rs の type EncodeCallback<T> の直前に追加
pub trait EncodeHandler<T>: Send + 'static {
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, Error>);
}

pub struct FnEncodeHandler<F>(F);

impl<F> FnEncodeHandler<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<T, F> EncodeHandler<T> for FnEncodeHandler<F>
where
    F: FnMut(Result<EncodedFrame<T>, Error>) + Send + 'static,
{
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, Error>) {
        (self.0)(result);
    }
}
```

### 2. `FnDecodeHandler` / `FnEncodeHandler` の設計上の注意点

- `F` フィールドは非公開 (`struct FnDecodeHandler<F>(F)` のタプルフィールドに可視性は付けられないが、意図的に直接アクセスさせない設計とする)
- コンストラクタは `FnDecodeHandler::new(closure)` で統一する。タプル構造体だが公開 `new` を提供することで将来的に内部構造を変更できる余地を残す
- `#[derive(Debug)]` は `F: FnMut(...)` に対して `Debug` を要求できないため付与しない
- `Clone` も `F: FnMut(...)` が `Clone` でないため付与しない
- **trait を `FnMut(...)` に対して直接 blanket impl しない理由**: `impl<T, F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static> DecodeHandler<T> for F` のような blanket impl は、`Encoder::new(|result| { ... })` と書いたときに `F` の推論が複雑化しコンパイルエラーになりやすい。専用ラッパー型を挟むことで API が明示的になり、型推論の負荷を下げられる
- **パニック安全性**: 現状の `FnMut` と同様に、Video Toolbox のコールバックスレッド（FFI 越し）でのパニックは FFI 境界を越えた stack unwinding により未定義動作となる。本 issue では対策を追加しない（現状の振る舞いを維持する）
- **命名根拠**: `DecodeHandler` / `EncodeHandler` は対応する `Decoder` / `Encoder` との関連を明確にするため動詞原型を採用する

### 3. `new()` 関数のシグネチャ変更と内部格納

```rust
// 変更前（decoder.rs:93）
pub fn new<F>(config: DecoderConfig<'_>, on_decoded: F) -> Result<Self, Error>
where
    F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static,

// 変更後（DecodeHandler<T> は Send + 'static の supertrait を持つため追加境界不要）
pub fn new<H>(config: DecoderConfig<'_>, on_decoded: H) -> Result<Self, Error>
where
    H: DecodeHandler<T>,
```

`new()` 内のコールバック格納コードは型エイリアス経由で自動的に適切な trait object を構築するため、実コードの変更は不要:

```rust
// この行は変更不要（BoxDecodeCallback<T> の定義が trait ベースに変わることで自動的に追従する）
let callback = Box::new(Box::new(on_decoded) as BoxDecodeCallback<T>);
```

`Encoder::new` も同様に変更する。

### 4. 内部実装の変更（エイリアス、`invoke_callback`、`callback_from_ref_con`、セッション再作成）

**型エイリアスの変更**:

```rust
// decoder.rs
type DecodeCallback<T> = dyn DecodeHandler<T> + Send + 'static;
type BoxDecodeCallback<T> = Box<DecodeCallback<T>>;

// encoder.rs
type EncodeCallback<T> = dyn EncodeHandler<T> + Send + 'static;
type BoxEncodedCallback<T> = Box<EncodeCallback<T>>;
```

**`invoke_callback` の変更**（`decoder.rs:400-406`、`encoder.rs:875-881`）:

```rust
// 変更前
fn invoke_callback(callback: &mut BoxDecodeCallback<T>, result: Result<DecodedFrame<T>, Error>) {
    let callback = callback.as_mut();
    (callback)(result);
}

// 変更後（.as_mut() 不要、自動 deref で呼び出せる）
fn invoke_callback(callback: &mut BoxDecodeCallback<T>, result: Result<DecodedFrame<T>, Error>) {
    callback.on_decoded(result);
}
```

**`callback_from_ref_con` の戻り型**（`decoder.rs:389-398`、`encoder.rs:864-873`）:
- 戻り型は `Option<&'a mut BoxDecodeCallback<T>>` / `Option<&'a mut BoxEncodedCallback<T>>` のまま。型エイリアス経由で自動的に追従する

**`update_format()` / `reconfigure()` 内のコールバック再利用**:
- `Decoder::update_format()` (`decoder.rs:143-182`) と `Encoder::reconfigure()` (`encoder.rs:195-215`) はセッション再作成時に `self.callback.as_ref()` を `create_decompression_session` / `create_compression_session` に渡している。これらの内部関数の仮引数型は `BoxDecodeCallback<T>` / `BoxEncodedCallback<T>` のまま。型エイリアス変更により自動的に追従するため特別な対応は不要

**doc comment の更新**:
- `decoder.rs:82` の `FnMut(Result<DecodedFrame<T>, Error>)` → `DecodeHandler<T>` に書き換え
- `encoder.rs:162` の `FnMut(Result<EncodedFrame<T>, Error>)` → `EncodeHandler<T>` に書き換え

### 5. `src/lib.rs` のエクスポート追加

```rust
// lib.rs の pub use decoder 行に追加
pub use decoder::{..., DecodeHandler, FnDecodeHandler};
// lib.rs の pub use encoder 行に追加
pub use encoder::{..., EncodeHandler, FnEncodeHandler};
```

### 6. テストとサンプルの移行

**テストファイル**（`tests/test_decoder.rs`、`tests/test_encoder.rs`）:
- `Decoder::new(config, |_| {})` → `Decoder::new(config, FnDecodeHandler::new(|_| {}))` に変更
- `Encoder::new(config, move |result| { ... })` → `Encoder::new(config, FnEncodeHandler::new(move |result| { ... }))` に変更
- 合計約 20 箇所の `new()` 呼び出しを機械的に書き換える

**サンプル**（`examples/raden_to_mp4.rs:334`）:
- `Encoder::new(config, move |result| { ... })` → `Encoder::new(config, FnEncodeHandler::new(move |result| { ... }))` に変更

### 7. テスト戦略

- **PBT**: なし。`DecodeHandler` / `EncodeHandler` trait 自体はロジックを持たず、`FnDecodeHandler` / `FnEncodeHandler` は内部の `FnMut` に委譲するだけの薄いラッパーであり、PBT で生成検証するプロパティがない
- **単体テスト**: 特になし。既存のエラーケーステスト（`test_decoder.rs` の拒否テスト、`test_encoder.rs` の `InsufficientFrameData` / `PixelFormatMismatch` テスト等）が `FnDecodeHandler` / `FnEncodeHandler` ラッパー経由でも動作することを確認すれば十分
- **Fuzzing**: なし。今回の変更範囲では不要

### 8. 互換性

- 既存の `FnMut` クロージャは `FnDecodeHandler::new(closure)` / `FnEncodeHandler::new(closure)` でラップするだけで使える
- `Decoder::new` / `Encoder::new` のシグネチャ変更は破壊的変更（`[CHANGE]`）として扱う
- 新規の trait とラッパー構造体の追加は後方互換のある追加（`[ADD]`）として扱う

## 完了条件

- `DecodeHandler<T>` trait と `FnDecodeHandler<F>` ラッパーが `src/decoder.rs` に定義され、`src/lib.rs` からエクスポートされていること
- `EncodeHandler<T>` trait と `FnEncodeHandler<F>` ラッパーが `src/encoder.rs` に定義され、`src/lib.rs` からエクスポートされていること
- 新規公開型に `#![warn(missing_docs)]` に対応した rustdoc コメントが付与されていること
- `Decoder::new` が `H: DecodeHandler<T>` を受け取るように変更されていること
- `Encoder::new` が `H: EncodeHandler<T>` を受け取るように変更されていること
- 内部の型エイリアス `DecodeCallback<T>` / `EncodeCallback<T>` / `BoxDecodeCallback<T>` / `BoxEncodedCallback<T>` が trait ベースに変更されていること
- `invoke_callback` が `.as_mut()` なしの trait メソッド呼び出しに変更されていること
- `Decoder` / `Encoder` の doc comment の `FnMut` 記述が `DecodeHandler<T>` / `EncodeHandler<T>` に更新されていること
- `tests/test_decoder.rs` / `tests/test_encoder.rs` の全 `new()` 呼び出しが `FnDecodeHandler::new(...)` / `FnEncodeHandler::new(...)` ラッパー経由になっていること
- `examples/raden_to_mp4.rs` の `Encoder::new` 呼び出しが `FnEncodeHandler::new(...)` ラッパー経由になっていること
- `CHANGES.md` の `develop` セクションに `[ADD]` エントリ（trait / wrapper 追加）と `[CHANGE]` エントリ（`new()` シグネチャ変更）の両方が追記されていること

## 解決方法

以下の変更を実施した:

- `src/decoder.rs`: `DecodeHandler<T>` trait と `FnDecodeHandler<F>` ラッパーを追加し、型エイリアス (`DecodeCallback<T>`, `BoxDecodeCallback<T>`) を `dyn DecodeHandler<T>` ベースに変更。`new()` のシグネチャを `H: DecodeHandler<T>` に変更。`invoke_callback` を `.on_decoded(result)` 呼び出しに変更。doc comment を `DecodeHandler<T>` に更新
- `src/encoder.rs`: `EncodeHandler<T>` trait と `FnEncodeHandler<F>` ラッパーを追加し、型エイリアス (`EncodeCallback<T>`, `BoxEncodedCallback<T>`) を `dyn EncodeHandler<T>` ベースに変更。`new()` のシグネチャを `H: EncodeHandler<T>` に変更。`invoke_callback` を `.on_encoded(result)` 呼び出しに変更。doc comment を `EncodeHandler<T>` に更新
- `src/lib.rs`: `DecodeHandler`, `FnDecodeHandler`, `EncodeHandler`, `FnEncodeHandler` をエクスポート
- `tests/test_decoder.rs`: 全 `Decoder::new()` 呼び出しを `FnDecodeHandler::new(...)` 経由に変更
- `tests/test_encoder.rs`: 全 `Encoder::new()` 呼び出しを `FnEncodeHandler::new(...)` 経由に変更
- `examples/raden_to_mp4.rs`: `Encoder::new()` 呼び出しを `FnEncodeHandler::new(...)` 経由に変更
- `README.md`: コード例 3 箇所を wrapper 経由に更新
- `CHANGES.md`: `[ADD]` 2 エントリと `[CHANGE]` 2 エントリを追記

全 23 テストが成功し、clippy 警告もないことを確認した。
