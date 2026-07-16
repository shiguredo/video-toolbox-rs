# Encoder に kVTCompressionPropertyKey_DataRateLimits 対応を追加する

- Priority: High
- Created: 2026-07-16
- Completed: 2026-07-16
- Model: Fable 5
- Branch: feature/add-encoder-data-rate-limits

## 目的

Video Toolbox の rate control は `kVTCompressionPropertyKey_AverageBitRate` の指定だけでは
短期ウィンドウで大きくオーバーシュートする。実測では 720p30 HEVC を 500 kbps 指定で
エンコードすると平均 700 kbps 超・ピーク 1.1 Mbps の出力になり、WebRTC 用途 (suzume の
WHIP クライアント) でサーバー側の帯域上限 (REMB/TMMBR) を恒常的に超過してしまう。

`kVTCompressionPropertyKey_DataRateLimits` は「指定秒数のウィンドウあたりの総バイト数」の
ハードリミットで、`AverageBitRate` と併用することで短期オーバーシュートを抑えられる
(VTCompressionProperties.h の discussion 参照)。libwebrtc の VideoToolbox H.264 エンコーダーも
target bitrate 変更のたびに `DataRateLimits = [target_bps / 8 * 1.5 bytes, 1 秒]` を設定している。

## 優先度根拠

suzume (WHIP クライアント) の帯域超過問題の解決に直接必要なため High。
closed/0043 で「必要になった時点で別 issue で追加する」とスコープ外にされていた項目。

## 現状

- `EncoderConfig` / `ReconfigureParams` に data rate limits に相当するフィールドが無い
- CFArray を生成するヘルパーが `types.rs` に無い (CFNumber / CFDictionary のみ)
- bindings には `CFArrayCreate` / `kCFTypeArrayCallBacks` /
  `kVTCompressionPropertyKey_DataRateLimits` が既に含まれている

## 解決方法

- `DataRateLimit { bytes: u64, window: Duration }` 型を追加する
  - SDK 仕様上「0〜2 個のハードリミット」なので個数は最大 2 に検証する
- `EncoderConfig::data_rate_limits: Vec<DataRateLimit>` を追加する (空 = 未設定)
- `ReconfigureParams::data_rate_limits: Option<Vec<DataRateLimit>>` を追加する
  (None = 現在値を維持、Some(空 Vec) = 上限解除)
- `types.rs` に `cf_array` ヘルパーを追加する
- プロパティは「bytes (CFNumber SInt64), seconds (CFNumber Float64)」を交互に並べた
  CFArray として `VTSessionSetProperties` に渡す

## テスト方針

- 検証エラー (bytes = 0 / window = 0 / 3 個以上) の単体テスト
- reconfigure で `config()` に反映されることのテスト
- 実エンコードで「ウィンドウあたりの出力バイト数が上限に収まる」ことの実測テスト
  (ハードウェア依存の揺らぎを考慮して上限には余裕を持たせる)
