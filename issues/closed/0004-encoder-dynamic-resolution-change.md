# エンコーダーが映像サイズの動的変更に対応していない

## 概要

現在のエンコーダーは `Encoder::new()` 時に `EncoderConfig` の `width` / `height` から `VTCompressionSession` を一度だけ作成し、それを使い続ける。ストリーム中に解像度を変更する手段がない。

## 背景

WebRTC や適応ビットレートストリーミングでは、ネットワーク状況に応じてエンコード解像度を動的に変更することがある。デコーダー側は `Decoder::update_format()` で対応済みだが、エンコーダー側には同等の機能がない。

## デコーダーとの違い

- デコーダーには `VTDecompressionSessionCanAcceptFormatDescription()` があり、既存セッションを流用できるか判定できる
- エンコーダーには同等の API が存在しない。`VTCompressionSession` は作成時に `width` / `height` を固定するため、解像度変更は常にセッションの破棄と再作成が必要

## 現在の問題

- `Encoder` は `session` を初期化時に固定している
- エンコード中に解像度を変更する手段がない
- 解像度を変えたい場合は `Encoder` を破棄して新しいインスタンスを作り直す必要がある

## 対応方針

`Encoder` にセッションを再作成するメソッドを追加する。

### API 案

```rust
impl Encoder {
    /// 新しい設定でエンコーダーを再作成する
    ///
    /// 未出力フレームをフラッシュした後、既存のセッションを破棄して
    /// 新しい設定でセッションを再作成する。
    pub fn reconfigure(&mut self, config: EncoderConfig) -> Result<(), Error> { ... }
}
```

### 処理フロー

1. `VTCompressionSessionCompleteFrames()` で未出力フレームをフラッシュする
2. 既存のセッションを `VTCompressionSessionInvalidate` + `CFRelease` で破棄する
3. 新しい `EncoderConfig` でセッションを再作成する
4. 内部状態 (`next_input_pts` / `next_output_pts` / `output_frames`) をリセットする

### 注意事項

- エンコーダーのセッション再作成はデコーダーと比較してコストが大きい
- フラッシュ前の未出力フレームは `next_frame()` で取得可能にする必要がある
- `pixel_format` の変更も許容するかは検討が必要

## 変更対象ファイル

- `src/lib.rs`: `Encoder::reconfigure()` メソッド追加
- `README.md`: エンコードセクションに使用例追加
- `CHANGES.md`: 変更履歴追加

## 完了内容

### 実装した変更

1. `Encoder::new()` のセッション作成ロジックを `create_compression_session()` ヘルパーに抽出
2. `Encoder::reconfigure()` メソッドを追加
   - `finish()` で未出力フレームをフラッシュ
   - 既存セッションを `VTCompressionSessionInvalidate` + `CFRelease` で破棄
   - チャネルを再作成して新しい `EncoderConfig` でセッションを再作成
   - 内部状態 (`next_input_pts` / `next_output_pts` / `output_frames`) をリセット
3. `Encoder` 構造体の `encoded_frame_tx` フィールドから `#[expect(dead_code)]` を削除
   - `reconfigure()` で直接参照されるようになったため不要
4. `README.md` にエンコードセクションで `reconfigure()` の使用例を追加
5. `CHANGES.md` に `[ADD] Encoder::reconfigure() メソッドを追加する` を記載
