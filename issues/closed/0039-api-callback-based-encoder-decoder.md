# Encoder / Decoder を WebCodecs 風コールバックベース API に変更する

Created: 2026-04-21  
Completed: 2026-04-21  
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

## 解決方法

### Encoder / Decoder 構造の刷新

- `Encoder::new<F>(callback: F)` / `Decoder::new<F>(callback: F)` を `FnMut(Result<Frame, Error>) + 'static` で受け取る形に変更し、内部で `Box<dyn FnMut(..)>` として保持する
- セッションと設定を `Option<...>` に変更し、`EncoderState` / `DecoderState` (`Unconfigured` / `Configured` / `Closed`) で状態遷移を管理
- `Error::InvalidState { operation, state }` を追加し、未設定・close 済みでの操作を明示的にエラーとする

### 出力コールバックの呼び出し規約

- エンコーダーは従来の VT コールバック → `mpsc::Sender` 経路を維持したまま、`encode()` / `flush()` の末尾で `drain_output()` を呼び、呼び出し元スレッドで同期的にユーザーコールバックを実行する
- デコーダーは `VTDecompressionSessionDecodeFrame` が同期完了するため、戻り直後に `CFRetain` 済みピクセルバッファを `DecodedFrame { pixel_buffer, timestamp }` として構築しコールバックへ渡す
- 成功・失敗は `Result<Frame, Error>` 一本化して `&mut self` を扱いやすくし、`Send` 境界を前提にせず `FnMut` と `'static` のみ要求する

### ライフサイクル API

- `Encoder::finish()` を `Encoder::flush()` にリネームし、`VTCompressionSessionCompleteFrames` 後に `drain_output()` を呼ぶ
- `Encoder::reset()` / `Encoder::close()` / `Encoder::state()` / `Encoder::encode_queue_size()` を新設 (WebCodecs の `reset` / `close` / `state` / `encodeQueueSize` 相当)
- `Encoder::reconfigure()` / `Decoder::update_format()` を廃止し、`configure()` の再呼び出しに集約する (2 回目以降は未出力フレームを破棄する契約)
- デコーダーは `VTDecompressionSessionCanAcceptFormatDescription` で既存セッション流用可否を判定する既存ロジックを `configure()` 内に集約
- デコーダーに `flush()` / `reset()` / `close()` / `state()` を追加

### `DecodedFrame` の再設計

- 旧 `DecodedFrame<'a>` enum / `I420Frame<'a>` / `Nv12Frame<'a>` の借用ベース設計を廃止し、`PixelBuffer` (CVPixelBuffer の CFRetain 所有ラッパー、`Send`) + `timestamp: i64` を持つ struct に変更
- `PixelBuffer::lock()` で `LockedPixelBuffer::{I420, Nv12}(view)` を取得し、プレーン参照は `I420View<'a>` / `Nv12View<'a>` の `Drop` で `CVPixelBufferUnlockBaseAddress` を自動解放する
- `PixelBuffer::as_ptr()` は video-device-rs の `PixelBuffer` と同形で、`Encoder::encode_pixel_buffer` に直接渡せる

### その他

- `allow_frame_reordering: false` 前提を強化するため、エンコード出力の `HashMap` + PTS 整列ロジックを削除し、VT からの到着順でそのままコールバックへ通知する
- `EncodedFrame.timestamp` を公開 (旧 private `pts` を改名)
- `Decoder::decode` のシグネチャを `Result<Option<DecodedFrame<'_>>, Error>` から `Result<(), Error>` に変更し、WebCodecs の `EncodedVideoChunk.timestamp` に対応する `timestamp: i64` 引数を追加
- `examples/raden_to_mp4.rs` を `Rc<RefCell<WriterState>>` を共有するコールバック形式に書き換え、H.264 / H.265 双方で MP4 生成を確認
- `tests/test_lib.rs` を新 API に合わせて再構成し、state 遷移・`InvalidState`・`configure` 再呼び出し・`reset` 後の Unconfigured 復帰を追加検証 (合計 24 件)
