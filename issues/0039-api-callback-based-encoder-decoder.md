# Encoder / Decoder を WebCodecs 風コールバックベース API に変更する

Created: 2026-04-21  
Model: Opus 4.7

## なぜこの対応が必要か

現在の `Encoder` / `Decoder` は pull 型 (`encode()` で投入 → `next_frame()` でポーリング) であり、リアルタイム配信やストリーミング用途ではループで `next_frame()` を呼び続ける必要があり、記述量が増え呼び忘れによるフレーム滞留が発生しやすい。リアルタイム用途を想定すると、フレームが生成され次第呼び出される push 型の API が自然である。

W3C WebCodecs API は push 型の設計を採用しており、以下の観点で参考になる:

- フレーム単位のライフサイクル (`configure` / `encode` / `flush` / `reset` / `close`) が明確
- 未設定・処理中・破棄済みを表す `state` プロパティでライフサイクル逸脱を検出できる
- `encodeQueueSize` で未処理キュー量を公開できる

ただし WebCodecs は JavaScript の制約で成功通知 (`output`) と失敗通知 (`error`) の 2 本のコールバックに分けているが、Rust は `Result` 型があるため 1 本の `FnMut(Result<T, Error>)` に統一できる。2 本に分けると同一の可変状態 (`&mut`) を両方のクロージャから触る際に `Rc<RefCell<>>` / `Arc<Mutex<>>` が必須となり煩雑になるため、1 本化が Rust idiom に合う。

破壊的変更を許容して API を刷新する。姉妹プロジェクト (nvcodec-rs / vpl-rs) への水平展開は本 issue のスコープ外とし、別 issue で追随する。

## 現状

### Encoder

- `Encoder::new(config)` でセッション作成
- `Encoder::encode(&frame, &options)` で投入、VT コールバックは `mpsc::Sender` に送る
- `Encoder::next_frame()` で PTS 順 (HashMap 整列) に pull 取得
- `Encoder::finish()` で未出力フラッシュ、その後 `next_frame()` を再度回す必要あり
- `Encoder::reconfigure(config)` で動的再設定 (内部で未出力 flush + セッション再作成)

### Decoder

- `Decoder::new(config)` でセッション作成
- `Decoder::decode(&data)` が `Result<Option<DecodedFrame<'_>>, Error>` を返し同期デコード
- `DecodedFrame<'a>` は CVPixelBuffer のロック期間に紐づくライフタイム借用
- `Decoder::update_format(codec)` でパラメータセット再設定

## 望ましい対応の方向

### 共通

- `Error::InvalidState { operation, state }` バリアント追加
- `EncoderState` / `DecoderState` enum を追加 (`Unconfigured` / `Configured` / `Closed`)

### Encoder

```rust
impl Encoder {
    pub fn new<F>(callback: F) -> Self
    where
        F: FnMut(Result<EncodedFrame, Error>) + 'static;

    pub fn configure(&mut self, config: EncoderConfig) -> Result<(), Error>;
    pub fn encode(&mut self, frame: &FrameData<'_>, opts: &EncodeOptions) -> Result<(), Error>;
    pub unsafe fn encode_pixel_buffer(&mut self, pb: *mut c_void, opts: &EncodeOptions) -> Result<(), Error>;
    pub fn flush(&mut self) -> Result<(), Error>;
    pub fn reset(&mut self) -> Result<(), Error>;
    pub fn close(&mut self) -> Result<(), Error>;
    pub fn state(&self) -> EncoderState;
    pub fn encode_queue_size(&self) -> usize;
}
```

- `next_frame` / `finish` / `reconfigure` を廃止
- `EncodedFrame.timestamp` を公開 (旧 private `pts` を改名)
- VT コールバックからの受信は現状の mpsc を維持 (スレッド安全)
- `encode` / `flush` 末尾で内部 mpsc を drain し、呼び出し元スレッドで callback を実行する
- `allow_frame_reordering: false` 前提の強化として HashMap + PTS 整列ロジックを削除 (B フレームを後日扱うならその時に別途設計)

### Decoder

```rust
impl Decoder {
    pub fn new<F>(callback: F) -> Self
    where
        F: FnMut(Result<DecodedFrame, Error>) + 'static;

    pub fn configure(&mut self, config: DecoderConfig<'_>) -> Result<(), Error>;
    pub fn decode(&mut self, data: &[u8]) -> Result<(), Error>;
    pub fn flush(&mut self) -> Result<(), Error>;
    pub fn reset(&mut self) -> Result<(), Error>;
    pub fn close(&mut self) -> Result<(), Error>;
    pub fn state(&self) -> DecoderState;
}

pub struct DecodedFrame {
    pub pixel_buffer: PixelBuffer,
    pub timestamp: i64,
}

pub struct PixelBuffer { /* CVPixelBuffer の所有ラッパー */ }
pub enum LockedPixelBuffer<'a> { I420(I420View<'a>), Nv12(Nv12View<'a>) }
```

- `update_format` を廃止し `configure` の再呼び出しに集約
- `DecodedFrame<'a>` / `I420Frame<'a>` / `Nv12Frame<'a>` の借用ライフタイム設計を廃止
- 代わりに `PixelBuffer` (CVPixelBuffer を CFRetain した所有ラッパー) を callback に渡す
- plane 参照は `pixel_buffer.lock()?` で取得する `LockedPixelBuffer<'_>` 経由で行う
- `PixelBuffer::as_ptr()` は video-device-rs の `PixelBuffer` と同形 (将来の直接受け渡しに備える)

### 設計方針

- インスタンス化は `new(callback)` + `configure(config)` の 2 段階 (W3C 準拠)
- 2 回目以降の `configure` はユーザーが事前に `flush()` を呼ぶ契約。未出力フレームは破棄 (W3C `reset` 相当の挙動)
- コールバックは `encode()` / `flush()` / `decode()` の呼び出し元スレッドで同期的に呼ぶ。`Send` 不要、`'static` 必須
- VT 内部スレッドから直接 user callback を呼ばないことで、ユーザーが `FnMut` で状態を自然に可変キャプチャできる
- 非同期エラー (VT 側コールバックからの例外) は `Err(Error::..)` として callback に渡す

## 完了条件

- 上記 API 仕様通りに `src/lib.rs` を改修する
- `examples/raden_to_mp4.rs` を新 API で書き換え、H.264 / H.265 双方で MP4 出力が成功すること
- `tests/test_lib.rs` を新 API に合わせて更新し、以下を検証する:
  - `Unconfigured` 状態での `encode` / `decode` が `Error::InvalidState` を返す
  - `Closed` 状態での各操作が `Error::InvalidState` を返す
  - `configure` → `encode` × N → `flush` で callback 到達数が期待通り
  - `flush` → `configure(new)` → `encode` で正常継続
  - `reset` 後に `Unconfigured` に戻り再 `configure` が必要
  - Decoder パイプラインでのフレーム数・解像度一致
- `CHANGES.md` の `## develop` に `[ADD]` / `[CHANGE]` を種別順で追加
- `README.md` のサンプルを callback スタイルに更新
- `make fmt` / `make clippy` (警告ゼロ) / `make test` が通る
