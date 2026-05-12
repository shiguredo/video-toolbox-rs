# Encoder/Decoder の型パラメータをユーザーデータ型からハンドラ型に変更する

Created: 2026-05-12
Completed: 2026-05-12
Model: deepseek-v4-pro

## なぜこの対応が必要か

issue 0039 で `EncodeHandler<T>` / `DecodeHandler<T>` trait を導入したが、型パラメータ `T` はユーザーデータ型のままであり、ハンドラは `dyn EncodeHandler<T>` に型消去して二重 Box で格納している。この設計には以下の問題がある:

1. **二重 Box による不要なヒープ割り当てと間接参照**: `dyn Trait` は fat pointer (2 ワード) のため FFI の `*mut c_void` (1 ワード) に直接渡せず外側の Box が必要になっている。ハンドラを具象型で持てばサイズがコンパイル時に確定するため、単一の Box で十分になる
2. **動的ディスパッチのオーバーヘッド**: コールバック呼び出しのたびに vtable 経由の間接呼び出しが発生する。具象型を保持すればモノモーフィゼーションにより静的ディスパッチになる
3. **型パラメータの意味の不明瞭さ**: `Encoder<u64>` と書いたとき、`u64` が「ユーザーデータ」であることは型シグネチャからは分からない。`Encoder<MyHandler>` であれば「このエンコーダーはこのハンドラで動作する」と型レベルで自明になる
4. **ハンドラの型情報の喪失**: 現在はハンドラが trait object に消去されるため、ハンドラに状態取得メソッドを追加しても `Encoder` 経由ではアクセスできない。具象型で持てば `encoder.handler()` のような accessor で具象型の全メソッドにアクセス可能になる

## 現状

### `src/encoder.rs`

```rust
pub trait EncodeHandler<T>: Send + 'static {
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, Error>);
}

pub struct FnEncodeHandler<F>(F);

impl<T, F> EncodeHandler<T> for FnEncodeHandler<F>
where
    F: FnMut(Result<EncodedFrame<T>, Error>) + Send + 'static,
{ ... }

type EncodeCallback<T> = dyn EncodeHandler<T> + Send + 'static;
type BoxEncodedCallback<T> = Box<EncodeCallback<T>>;

pub struct Encoder<T: Send + 'static> {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    callback: Box<BoxEncodedCallback<T>>,
}

impl<T: Send + 'static> Encoder<T> {
    pub fn new<H>(config: EncoderConfig, on_encoded: H) -> Result<Self, Error>
    where
        H: EncodeHandler<T>,
    { ... }
}
```

### `src/decoder.rs`

```rust
pub trait DecodeHandler<T>: Send + 'static {
    fn on_decoded(&mut self, result: Result<DecodedFrame<T>, Error>);
}

pub struct FnDecodeHandler<F>(F);

impl<T, F> DecodeHandler<T> for FnDecodeHandler<F>
where
    F: FnMut(Result<DecodedFrame<T>, Error>) + Send + 'static,
{ ... }

type DecodeCallback<T> = dyn DecodeHandler<T> + Send + 'static;
type BoxDecodeCallback<T> = Box<DecodeCallback<T>>;

pub struct Decoder<T: Send + 'static> {
    description: sys::CMVideoFormatDescriptionRef,
    session: sys::VTDecompressionSessionRef,
    pixel_format: PixelFormat,
    callback: Box<BoxDecodeCallback<T>>,
}

impl<T: Send + 'static> Decoder<T> {
    pub fn new<H>(config: DecoderConfig<'_>, on_decoded: H) -> Result<Self, Error>
    where
        H: DecodeHandler<T>,
    { ... }
}
```

## 変更内容

### 1. trait 定義を associated type に変更する

```rust
// src/encoder.rs
pub trait EncodeHandler: Send + 'static {
    type UserData: Send + 'static;
    type Error: From<crate::Error> + Send + 'static = crate::Error;
    fn on_encoded(&mut self, result: Result<EncodedFrame<Self::UserData>, Self::Error>);
}

// src/decoder.rs
pub trait DecodeHandler: Send + 'static {
    type UserData: Send + 'static;
    type Error: From<crate::Error> + Send + 'static = crate::Error;
    fn on_decoded(&mut self, result: Result<DecodedFrame<Self::UserData>, Self::Error>);
}
```

**`type Error = crate::Error` デフォルトの理由**: `crate::Error` をデフォルトとすることで、`FnEncodeHandler` / `FnDecodeHandler` の振る舞いを変えずに `type Error` associated type を導入できる（`From<T> for T` の blanket impl により `crate::Error: From<crate::Error>` は自動的に満たされる）。カスタムエラー型が必要な場合のみユーザーが `type Error = MyError;` を指定すればよい。

### 2. FnEncodeHandler / FnDecodeHandler を更新する

クロージャの具象型をそのまま構造体の型パラメータにすると、ユーザーがクロージャ型を明示できない（unnameable type になる）。このため、クロージャを `Box<dyn FnMut(...)>` で保持し型パラメータを `T` と `E` の 2 つに抑える。`E` には `crate::Error` をデフォルト設定し、トレイトの `type Error = crate::Error` デフォルトと一貫性を持たせる:

```rust
// src/encoder.rs
// タプル構造体から通常の構造体に変更
pub struct FnEncodeHandler<T, E = crate::Error> {
    f: Box<dyn FnMut(Result<EncodedFrame<T>, E>) + Send + 'static>,
}

impl<T, E> FnEncodeHandler<T, E> {
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(Result<EncodedFrame<T>, E>) + Send + 'static,
    {
        Self { f: Box::new(f) }
    }
}

impl<T, E> EncodeHandler for FnEncodeHandler<T, E>
where
    T: Send + 'static,
    E: From<crate::Error> + Send + 'static,
{
    type UserData = T;
    type Error = E;
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, E>) {
        (self.f)(result);
    }
}
```

```rust
// src/decoder.rs
// タプル構造体から通常の構造体に変更
pub struct FnDecodeHandler<T, E = crate::Error> {
    f: Box<dyn FnMut(Result<DecodedFrame<T>, E>) + Send + 'static>,
}

impl<T, E> FnDecodeHandler<T, E> {
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(Result<DecodedFrame<T>, E>) + Send + 'static,
    {
        Self { f: Box::new(f) }
    }
}

impl<T, E> DecodeHandler for FnDecodeHandler<T, E>
where
    T: Send + 'static,
    E: From<crate::Error> + Send + 'static,
{
    type UserData = T;
    type Error = E;
    fn on_decoded(&mut self, result: Result<DecodedFrame<T>, E>) {
        (self.f)(result);
    }
}
```

**`fn new<F>` で型消去する理由**: クロージャ型 `F` を構造体の型パラメータから外し `Box<dyn FnMut(...)>` に消去することで、`Encoder<FnEncodeHandler<u64, MyError>>` のように具体的な型名を書けるようになる。`F` は `new()` のジェネリックパラメータとしてのみ使われ、`Box::new(f)` の時点で型消去される。動的ディスパッチが発生するが、これはハンドラを trait object で持っていた従来設計と同じである。静的ディスパッチが必要な場合は `EncodeHandler` trait を直接実装すればよい。

`T` と `E` は `Box<dyn FnMut(...)>` の型引数で使用されているため PhantomData は不要。型推論の具体例は「テストとサンプルの移行例」セクションを参照。

### 3. Encoder / Decoder の構造体を変更する

```rust
// src/encoder.rs
pub struct Encoder<H: EncodeHandler> {
    session: sys::VTCompressionSessionRef,
    config: EncoderConfig,
    next_input_pts: i64,
    handler: Box<H>,
}

// src/decoder.rs
pub struct Decoder<H: DecodeHandler> {
    description: sys::CMVideoFormatDescriptionRef,
    session: sys::VTDecompressionSessionRef,
    pixel_format: PixelFormat,
    handler: Box<H>,
}
```

**単一 Box の理由**: `Encoder` / `Decoder` が move されても `handler` のヒープアドレスは不変であり、FFI の `ref_con` に渡したポインタが有効であり続ける。`H` は具象型（サイズ既知）のため thin pointer となり、`*mut c_void` へのキャストに二重 Box は不要。

**FFI ポインタ受け渡し**: 既存コードと同じ参照→生ポインタ→復元パターンを維持する。`create_compression_session` に `handler as *const H as *mut c_void` を渡し、コールバック内で `&mut *ref_con.cast::<H>()` で復元する。SAFETY コメントで以下を明記すること: (1) `Box<H>` のヒープアドレスが `Encoder` の生存期間中不変であること、(2) FFI コールバックが `&mut H` で排他的にアクセスすること。

### 4. `new()` のシグネチャ変更

```rust
// src/encoder.rs
impl<H: EncodeHandler> Encoder<H> {
    pub fn new(config: EncoderConfig, handler: H) -> Result<Self, Error> {
        let handler = Box::new(handler);
        let session = unsafe { Self::create_compression_session(&config, handler.as_ref())? };
        Ok(Self { session, config, next_input_pts: 0, handler })
    }
}

// src/decoder.rs
impl<H: DecodeHandler> Decoder<H> {
    pub fn new(config: DecoderConfig<'_>, handler: H) -> Result<Self, Error> {
        let handler = Box::new(handler);
        // ...
    }
}
```

### 5. FFI コールバック関連の変更

```rust
// callback_from_ref_con の変更 — 戻り型が &mut BoxEncodedCallback<T> から &mut H に変わる
// Box<H> のヒープアドレスは不変なので、コールバック呼び出し時に有効なポインタとして復元できる
unsafe fn callback_from_ref_con<'a>(
    ref_con: *mut c_void,
    callback_name: &'static str,
) -> Option<&'a mut H> {
    if ref_con.is_null() {
        log::error!("{callback_name}: ref_con is null");
        return None;
    }
    Some(unsafe { &mut *ref_con.cast::<H>() })
}

// invoke_callback の変更 — シグネチャが具象型 H になり、型消去が不要になる
// H::Error: From<crate::Error> バウンドにより、コールバック内のエラー型変換が可能
fn invoke_callback(handler: &mut H, result: Result<EncodedFrame<H::UserData>, H::Error>) {
    handler.on_encoded(result);
}
```

`process_encoded_output` 内の全エラーパスでの変更（`callback` → `handler`、`.into()` 追加）:

```rust
// 7 箇所のエラーハンドリング箇所すべてで Err(e.into()) に変更
// 例:
if let Err(e) = Error::check(status, callback_name) {
    log::error!("{e}");
    Self::invoke_callback(handler, Err(e.into()));  // e.into() で crate::Error → H::Error
    return;
}

// 正常系の invoke_callback 呼び出し:
Self::invoke_callback(handler, Ok(frame));
// frame 構築時の user_data: H::UserData も追従
```

デコーダー側 `output_callback` も同様に `callback` → `handler` に変数名を変更し、3 箇所のエラーパスで `.into()` を追加する。`callback_from_ref_con` の戻り型は `&mut H`、`invoke_callback` のシグネチャは `&mut H, Result<DecodedFrame<H::UserData>, H::Error>` に変更。`create_decompression_session` 内の `VTDecompressionOutputCallbackRecord` を保持するローカル変数 `callback` は、引数名との混同を避けるため `record` にリネームする。

### 6. 公開メソッドのシグネチャ変更

`encode()`、`encode_pixel_buffer()`、`decode()` の `user_data` パラメータの型が `T` から `H::UserData` に変わる:

```rust
// src/encoder.rs — encode()
pub fn encode(
    &mut self,
    frame: &FrameData<'_>,
    options: &EncodeOptions,
    user_data: H::UserData,
) -> Result<(), Error>

// src/encoder.rs — encode_pixel_buffer()
pub unsafe fn encode_pixel_buffer(
    &mut self,
    pixel_buffer_ptr: *mut c_void,
    options: &EncodeOptions,
    user_data: H::UserData,
) -> Result<(), Error>

// src/decoder.rs — decode()
pub fn decode(&mut self, data: &[u8], user_data: H::UserData) -> Result<(), Error>
```

内部の `Box::into_raw(Box::new(user_data)).cast::<c_void>()` の `T` → `H::UserData` への置き換え、およびエラーパスでの `Box::from_raw(...cast::<T>())` → `Box::from_raw(...cast::<H::UserData>())` への置き換えも必要。

`encode()` / `decode()` の戻り値は `Result<(), crate::Error>` のまま（同期的検証エラー）。非同期コールバックのエラーのみ `H::Error` に `.into()` 変換される。

### 7. 内部型の `T` を `H::UserData` に、`Error` を `H::Error` に置き換える

- `take_user_data` の戻り型: `Option<T>` → `Option<H::UserData>`（NULL チェックは変更不要）
- `take_pending_decode` の戻り型: `Option<Box<PendingDecode<T>>>` → `Option<Box<PendingDecode<H::UserData>>>`
- `PendingDecode<T>` → `PendingDecode<H::UserData>`
- `EncodedFrame<T>` → `EncodedFrame<H::UserData>`（使用箇所の型引数のみ変更。struct 定義の型パラメータ `T` は維持する）
- `DecodedFrame<T>` → `DecodedFrame<H::UserData>`（同上。enum 定義の型パラメータ `T` は維持する）
- `process_encoded_output` 内の `T` 参照 → `H::UserData`
- コールバックに渡す `Result` のエラー型: `Error` → `H::Error`。`.into()` 追加箇所は Section 5 参照

`type Error: From<crate::Error>` バウンドは FFI コールバック内で生成される `crate::Error` を変換するために必要。orphan rule により、クレート外のエラー型（`anyhow::Error` 等）を直接 `type Error` に指定するには newtype ラッパーが必要になる。

### 8. 不要な型エイリアスの削除

以下の型エイリアスは不要になるため削除する:

- `type EncodeCallback<T>`
- `type BoxEncodedCallback<T>`
- `type DecodeCallback<T>`
- `type BoxDecodeCallback<T>`

### 9. `Send` の unsafe impl、`Drop` 実装の更新

```rust
// Send 実装 — 既存の `unsafe impl<T: Send + 'static> Send for Encoder<T>` を削除し、以下に置き換え
// H: Send は EncodeHandler の supertrait で保証される
// SAFETY: VTCompressionSession は内部でスレッドセーフに管理されている。
// handler は Box<H> でヒープに隔離されており、FFI コールバックは &mut H で排他的に借用する。
unsafe impl<H: EncodeHandler> Send for Encoder<H> {}
unsafe impl<H: DecodeHandler> Send for Decoder<H> {}

// Sync は実装しない。VT コールバックが &mut H を要求し、同時アクセスが許容されないため。

// Drop 実装 — 型パラメータを変更
// 変更前: impl<T: Send + 'static> Drop for Encoder<T>
// 変更後: impl<H: EncodeHandler> Drop for Encoder<H>
impl<H: EncodeHandler> Drop for Encoder<H> {
    fn drop(&mut self) {
        unsafe {
            sys::VTCompressionSessionInvalidate(self.session);
            sys::CFRelease(self.session as *const c_void);
        }
    }
}
// Decoder の Drop 実装も同様に impl<H: DecodeHandler> Drop for Decoder<H> に変更
```

### 10. `create_compression_session` / `create_decompression_session` の引数変更

```rust
// src/encoder.rs — 変更前
unsafe fn create_compression_session(
    config: &EncoderConfig,
    callback_ref_con: &BoxEncodedCallback<T>,
) -> Result<sys::VTCompressionSessionRef, Error>

// src/encoder.rs — 変更後
unsafe fn create_compression_session(
    config: &EncoderConfig,
    handler: &H,
) -> Result<sys::VTCompressionSessionRef, Error>

// src/decoder.rs — 変更前
unsafe fn create_decompression_session(
    description: sys::CMVideoFormatDescriptionRef,
    pixel_format: PixelFormat,
    callback_ref_con: &BoxDecodeCallback<T>,
) -> Result<sys::VTDecompressionSessionRef, Error>

// src/decoder.rs — 変更後
unsafe fn create_decompression_session(
    description: sys::CMVideoFormatDescriptionRef,
    pixel_format: PixelFormat,
    handler: &H,
) -> Result<sys::VTDecompressionSessionRef, Error>
```

FFI に渡すポインタは `handler as *const H as *mut c_void`。デコーダー側の `VTDecompressionOutputCallbackRecord` 構築コードも `callback_ref_con` → `handler` に変数名変更。

### 11. `reconfigure()` / `update_format()` の更新

`self.handler.as_ref()` を `create_compression_session` / `create_decompression_session` に渡す。`Box<H>` のヒープアドレスは不変のため、セッション再作成後もポインタは有効。構造変更のみでロジックの変更は不要。

### 12. `handler()` accessor の追加

具象型 `H` を保持することの主要な動機の一つ。ただし FFI コールバックが `&mut H` としてハンドラにアクセスするため、アクセサの設計には注意が必要:

```rust
impl<H: EncodeHandler> Encoder<H> {
    /// ハンドラへの参照を返す
    ///
    /// # Safety
    ///
    /// FFI コールバックは別スレッドで `&mut H` としてハンドラにアクセスするため、
    /// このメソッドが返す `&H` と競合すると未定義動作になる。
    /// 呼び出し側は本メソッドを以下のいずれかのタイミングでのみ呼ぶこと:
    /// - `finish()` 完了後、すべてのコールバックが処理された後
    /// - `Encoder` が一切のエンコード中でないことが保証できる場合
    pub unsafe fn handler(&self) -> &H {
        &self.handler
    }
}

// デコーダー側も同様
impl<H: DecodeHandler> Decoder<H> {
    pub unsafe fn handler(&self) -> &H {
        &self.handler
    }
}
```

`handler_mut()` は提供しない。FFI コールバックが `&mut H` を持つ状況でさらにユーザーにも `&mut H` を提供すると、aliased `&mut` が確実に発生するため unsafe にしても実用が難しい。

## 変更対象ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `src/encoder.rs` | trait 定義変更、struct 変更 (`callback` → `handler`)、型エイリアス削除、FFI コールバック関連更新、`encode()` / `encode_pixel_buffer()` シグネチャ変更、`take_user_data` / `callback_from_ref_con` / `invoke_callback` 変更、`process_encoded_output` の全エラーパスに `.into()` 追加、`create_compression_session` 引数変更、`reconfigure()` 変数名変更、`Send` / `Drop` impl 更新、handler accessor 追加 |
| `src/decoder.rs` | trait 定義変更、struct 変更 (`callback` → `handler`)、型エイリアス削除、FFI コールバック関連更新、`decode()` シグネチャ変更、`take_pending_decode` / `callback_from_ref_con` / `invoke_callback` 変更、`output_callback` の全エラーパスに `.into()` 追加、`create_decompression_session` 引数変更、`update_format()` 変数名変更、`Send` / `Drop` impl 更新、handler accessor 追加、`PendingDecode<T>` → `PendingDecode<H::UserData>` |
| `src/lib.rs` | 変更不要の確認（型パラメータ変更は `pub use` 行に影響しない） |
| `tests/test_encoder.rs` | 全 `FnEncodeHandler::new(closure)` に型注釈追加（約 14 箇所）、`Encoder::<()>` の turbofish 削除 |
| `tests/test_decoder.rs` | 全 `FnDecodeHandler::new(closure)` に型注釈追加（約 6 箇所）、`Decoder::<()>` の turbofish 削除 |
| `examples/raden_to_mp4.rs` | `FnEncodeHandler::new(move |result: Result<EncodedFrame<u64>, VideoToolboxError>| { ... })` に型注釈追加 |
| `CHANGES.md` | `[CHANGE]` エントリ追記 |

## テスト戦略

- **PBT**: なし。encoder / decoder の PBT は現時点で存在せず、trait の associated type 化はロジックを持たないため新規 PBT も不要
- **単体テスト** (`tests/test_encoder.rs` / `tests/test_decoder.rs` に追加):
  - 既存テストが `FnEncodeHandler` / `FnDecodeHandler` 経由で型注釈付きクロージャでコンパイル・動作することを確認する
  - `unsafe fn handler(&self) -> &H` accessor が `finish()` 後に正しくハンドラ参照を返すことを確認するテスト
  - ユーザー定義の `EncodeHandler` を直接実装し、`type Error` にカスタムエラー型を指定した場合に `From<crate::Error>` 変換が正しく機能することを確認するテスト
- **Fuzzing**: なし。FFI ポインタキャストの型パラメータ変更のみであり、入力バイト列の構造は変わらないため不要
- **型レベル確認**: `type Error` のデフォルトが `crate::Error` であるため、`FnEncodeHandler::<u64>::new(closure)` のように `E` を省略しても既存テストがそのまま動作することを確認する

## 互換性

- `EncodeHandler<T>` / `DecodeHandler<T>` trait のシグネチャ変更（ジェネリックパラメータ `T` → associated type `UserData` + `Error`）は破壊的変更
- `FnEncodeHandler<F>` → `FnEncodeHandler<T, E = crate::Error>` は破壊的変更（型パラメータ変更、クロージャ引数への型注釈が必須になる）
- `Encoder<T>` → `Encoder<H: EncodeHandler>` は破壊的変更（型パラメータの意味が変わる）
- `Decoder<T>` → `Decoder<H: DecodeHandler>` は破壊的変更
- `Error` 関連: Section 7 に記載の orphan rule 制約により、`crate::Error` 以外のエラー型を直接 `type Error` に指定するには newtype ラッパーが必要。これは意図的な設計判断
- すべて `[CHANGE]` として扱う

### CHANGES.md エントリ草案

`## develop` セクションに以下を追記する。issue 0039 の trait 導入は本 issue の associated type 化に完全に包含されるため、0039 単体のエントリは追記せず 0040 の最終形のみを記載する:

```
- [CHANGE] `EncodeHandler` / `DecodeHandler` trait の型パラメータを associated type (`UserData` / `Error`) に変更し、ハンドラ具象型を `Encoder` / `Decoder` の型パラメータにする
  - `Encoder<T>` を `Encoder<H: EncodeHandler>` に、`Decoder<T>` を `Decoder<H: DecodeHandler>` に変更
  - `FnEncodeHandler<F>` を `FnEncodeHandler<T, E = crate::Error>` に、`FnDecodeHandler<F>` を `FnDecodeHandler<T, E = crate::Error>` に変更
  - 二重 Box を単一 Box に置き換え
  - @melpon
```

## テストとサンプルの移行例

### 型注釈が必要になるパターン

`FnEncodeHandler::new(closure)` のクロージャ引数には**型注釈が必須**になる。現在の `|_| {}` や `|result| { ... }` のような型注釈省略パターンは全箇所コンパイルエラーになる。以下は修正バリエーション:

```rust
// エラーのみ必要でユーザーデータ不要のケース (test_encoder.rs:208 等)
// 型パラメータ T=(), E=Error はクロージャ引数の型注釈から推論される
FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {})

// 明示的な型指定も可能:
Encoder::<FnEncodeHandler<(), Error>>::new(c, FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}))

// ユーザーデータとエラーの両方を扱うケース (test_encoder.rs:111)
FnEncodeHandler::new(|result: Result<EncodedFrame<u64>, Error>| {
    results.lock().expect("results mutex poisoned").push(result);
})

// サンプル (raden_to_mp4.rs:337)
FnEncodeHandler::new(move |result: Result<EncodedFrame<u64>, VideoToolboxError>| {
    if encoded_result_tx.send(result).is_err() { ... }
})

// デコーダー側 (test_decoder.rs:105)
FnDecodeHandler::new(|result: Result<DecodedFrame<u64>, Error>| {
    push_decode_event(&results, result);
})

// ターボフィッシュの変更
// 変更前: Encoder::<()>::new(c, FnEncodeHandler::new(|_| {}))
// 変更後: Encoder::new(c, FnEncodeHandler::new(|_: Result<EncodedFrame<()>, Error>| {}))
// H は FnEncodeHandler<(), Error> として推論されるため、Encoder::<H> の turbofish は不要
// 明示したい場合: Encoder::<FnEncodeHandler<(), Error>>::new(...)
```

### 影響を受ける箇所数

- `tests/test_encoder.rs`: 約 14 箇所の `Encoder::new` / `FnEncodeHandler::new` 呼び出し
- `tests/test_decoder.rs`: 約 6 箇所の `Decoder::new` / `FnDecodeHandler::new` 呼び出し
- `examples/raden_to_mp4.rs`: 1 箇所の `Encoder::new` / `FnEncodeHandler::new` 呼び出し

## 完了条件

変更内容の各 Section に記載したコード変更がすべて完了し、かつ以下を満たしていること:

- 新規/変更された公開 API（associated type `UserData` / `Error`、`FnEncodeHandler<T, E>`、`FnDecodeHandler<T, E>`、`unsafe fn handler()`）に `#![warn(missing_docs)]` 違反の警告が出ないこと
- `new()` 内の旧「二重 Box」説明コメント（`EncodeCallback<T>` は fat pointer であるため〜）が削除または更新されていること
- 既存の全テストがコンパイル・パスすること
- `CHANGES.md` の `develop` セクションに `[CHANGE]` エントリが追記されていること

## 解決方法

Issue の仕様に従い、以下の変更を実施した:

- `src/encoder.rs`: `EncodeHandler<T>` → `EncodeHandler` (associated types `UserData` + `Error`)、`FnEncodeHandler<F>` → `FnEncodeHandler<T, E = crate::Error>`、`Encoder<T>` → `Encoder<H: EncodeHandler>`、二重 Box 削除、型エイリアス削除、FFI コールバックの具象型化、全エラーパスに `.into()` 追加、`unsafe fn handler()` accessor 追加、SAFETY コメント更新
- `src/decoder.rs`: 同様の変更、`PendingDecode<T>` 使用箇所を `PendingDecode<H::UserData>` に変更、`create_decompression_session` 内のローカル変数 `callback` → `record` にリネーム
- `tests/test_encoder.rs`: 全クロージャに型注釈追加、`Encoder::<()>` ターボフィッシュ削除
- `tests/test_decoder.rs`: 全クロージャに型注釈追加、`Decoder::<()>` ターボフィッシュ削除
- `examples/raden_to_mp4.rs`: クロージャに型注釈追加
- `CHANGES.md`: `[CHANGE]` エントリ追記
- Rust 1.95 では associated type defaults が利用不可のため、trait の `type Error = crate::Error` デフォルトを削除し、構造体側のデフォルト `FnEncodeHandler<T, E = crate::Error>` で対応

全 23 テストパス、clippy 警告なし、`cargo doc` 警告なし。
