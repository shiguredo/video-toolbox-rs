# デコーダーが映像サイズの動的変更に対応していない

## 概要

現在のデコーダーは `Decoder::new()` 時に SPS/PPS (H.264/H.265) や width/height (VP9/AV1) から `CMVideoFormatDescription` と `VTDecompressionSession` を一度だけ作成し、それを使い続ける。ストリーム中に解像度が変更された場合に追従できない。

## 背景

H.264/H.265 のストリームでは SPS が更新されると解像度が変わる可能性がある。WebRTC や適応ビットレートストリーミングでは一般的なケースである。

Video Toolbox は `VTDecompressionSessionCanAcceptFormatDescription()` で新しい FormatDescription を現在のセッションが受け入れ可能か判定でき、不可能な場合はセッションを再作成する仕組みを提供している。

## 現在の問題

- `Decoder` は `description` と `session` を初期化時に固定している
- デコード中に新しい SPS/PPS を受け取ってもセッションを更新する手段がない
- 解像度が変わった場合、デコードに失敗するか不正な出力になる

## 対応方針

`Decoder` に解像度変更を検知してセッションを再作成する仕組みを追加する。

### API 案

```rust
impl Decoder {
    /// 新しいパラメータセットでデコーダーを更新する
    ///
    /// Video Toolbox の VTDecompressionSessionCanAcceptFormatDescription() で
    /// 現在のセッションが新しい FormatDescription を受け入れ可能か判定し、
    /// 不可能な場合はセッションを再作成する。
    pub fn update_format(&mut self, codec: DecoderCodec<'_>) -> Result<(), Error> { ... }
}
```

### 処理フロー

1. 新しい SPS/PPS/VPS から `CMVideoFormatDescription` を作成する
2. `VTDecompressionSessionCanAcceptFormatDescription()` で現在のセッションが受け入れ可能か判定する
3. 受け入れ可能な場合は `description` のみ更新する
4. 受け入れ不可能な場合は既存のセッションを破棄し、新しいセッションを作成する

## 変更対象ファイル

- `src/lib.rs`: `Decoder::update_format()` メソッド追加
- `src/sys.rs`: `VTDecompressionSessionCanAcceptFormatDescription` のバインディング追加（bindgen で自動生成済み）
- `README.md`: デコードセクションに使用例追加
- `CHANGES.md`: 変更履歴追加

## 完了内容

### 実装した変更

1. `Decoder::new()` のフォーマット記述作成とセッション作成を内部ヘルパーに抽出
   - `create_format_description()`: `DecoderCodec` から `CMVideoFormatDescription` を作成
   - `create_decompression_session()`: `CMVideoFormatDescription` と `PixelFormat` から `VTDecompressionSession` を作成
2. `Decoder::update_format()` メソッドを追加
   - `VTDecompressionSessionCanAcceptFormatDescription()` で受け入れ可能か判定
   - 受け入れ可能: `FormatDescription` のみ差し替え
   - 受け入れ不可能: セッションを `Invalidate` + `CFRelease` して再作成
3. `README.md` にデコードセクションで `update_format()` の使用例を追加
4. `CHANGES.md` に `[ADD] Decoder::update_format() メソッドを追加する` を記載
