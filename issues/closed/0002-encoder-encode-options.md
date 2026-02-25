# Encoder::encode() に EncodeOptions を追加する

## 概要

`Encoder::encode()` にフレーム単位のエンコードオプション `EncodeOptions` を追加し、キーフレーム生成の強制を可能にする。

## 背景

現在の `Encoder::encode()` は `VTCompressionSessionEncodeFrame` の `frameProperties` に `NULL` を渡しており、フレーム単位のオプション指定ができない。

## 新しい API

```rust
/// VTCompressionSessionEncodeFrame の frameProperties に指定するオプション
pub struct EncodeOptions {
    /// kVTEncodeFrameOptionKey_ForceKeyFrame
    pub force_key_frame: bool,
}

impl Encoder {
    pub fn encode(
        &mut self,
        y: &[u8],
        u: &[u8],
        v: &[u8],
        options: &EncodeOptions,
    ) -> Result<(), Error> { ... }
}
```

## 設計方針

- `EncodeOptions` struct を新設し `Default` を実装する (`force_key_frame: false`)
- `encode()` の引数に `options: &EncodeOptions` を追加する
- `force_key_frame` が `true` の場合、`kVTEncodeFrameOptionKey_ForceKeyFrame = kCFBooleanTrue` を含む `CFDictionary` を `frameProperties` に渡す
- `force_key_frame` が `false` の場合、`frameProperties` は `NULL` を渡す
- フィールド名は Video Toolbox の `kVTEncodeFrameOptionKey_ForceKeyFrame` に合わせる

## 変更対象ファイル

- `src/lib.rs`: `EncodeOptions` struct 追加、`encode()` の引数変更、`frameProperties` の組み立て
- `README.md`: エンコードセクションのコード例を更新
- `CHANGES.md`: 変更履歴を追加
