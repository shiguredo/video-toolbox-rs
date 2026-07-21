# `data_rate_limits` の空 Vec による上限解除が実際にレート制御へ反映されることを検証するテストを追加する

- Priority: Medium
- Created: 2026-07-16
- Updated: 2026-07-21
- Completed:
- Model: Fable 5
- Branch: feature/add-data-rate-limits-clear-behavior-test
- Polished: {YYYY-MM-DD}

## 目的

`ReconfigureParams::data_rate_limits` に `Some(空 Vec)` を渡すと「設定済みの上限を解除する」と rustdoc (`src/encoder.rs:174-176`) で公開 API の契約として明記しているが、この解除の実挙動を検証するテストが無い。

既存テスト `reconfigure_updates_data_rate_limits` (`tests/test_encoder.rs:525-544`) は「`reconfigure` が `Ok` を返し、`config()` が `None` になる」という記帳の確認のみで、Video Toolbox 側で上限が実際に外れた (出力レートが上限を超えて戻る) ことは観測していない。

解除の実装は空の CFArray を `kVTCompressionPropertyKey_DataRateLimits` に設定する方式 (`src/encoder.rs:440-442` と `push_data_rate_limits_property`) だが、一次資料の裏付けは間接的である:

- VTCompressionProperties.h の abstract は「Zero, one or two hard limits on data rate.」と 0 個 (空配列) をプロパティ値として許容している
- 一方 VTSession.h:79 は「Setting a property value to NULL restores the default value.」と、デフォルト復帰の正規手段を NULL 設定と規定している

空 CFArray の設定が「解除」ではなく「無効値として黙殺」される実装が存在しても、現行テストは通ってしまう。契約として rustdoc に断言している以上、実挙動での裏取りが必要。

## 優先度根拠

- 公開 API の契約 (rustdoc) に動作保証が無い状態であり、suzume (WHIP クライアント) 等の利用者が解除を前提に帯域制御を組むと気付けない不具合になり得る
- 既存の実測ハーネスを流用でき、追加コストは限定的
- 即座のバグではない (Apple Silicon での手元検証では設定・解除とも `Ok` が返る) ため Medium

## 現状

- `tests/test_encoder.rs:694-765` に `data_rate_limits_cap_windowed_output` があり、「合成フレーム負荷 + 750 kbps ハード上限 + 2 Mbps の `average_bitrate`」でウィンドウあたりの出力バイト数が上限に抑えられることを実測検証している
- 解除方向 (上限あり → 空 Vec で解除 → 出力レートが上限超えに戻る) の実測は存在しない

## 設計方針

`data_rate_limits_cap_windowed_output` の合成フレーム生成 (`synthetic_i420_frame`) とウィンドウ集計ロジックを流用し、次のシナリオを 1 本追加する:

1. 上限付き (750 kbps 相当) で `Encoder` を構築し、負荷フレームを 1〜2 秒ぶんエンコードして上限が効いていることを確認する
2. `reconfigure(ReconfigureParams { data_rate_limits: Some(Vec::new()), .. })` で解除する
3. 同じ負荷フレームをさらに 1〜2 秒ぶんエンコードし、解除後のウィンドウ合計バイト数が上限を明確に超えて増える (例: 上限の 2 倍以上) ことを確認する

閾値はハードウェア実装依存の揺れを考慮し、既存テストと同様に緩めのマージンを取る (`average_bitrate` 2 Mbps に対して上限 750 kbps なら、解除後は上限の 2 倍程度は確実に超える)。解除が黙殺される実装ならレートが上限付近に張り付いたままになるため、判定は安定して分離できる。

もし実測で「空 CFArray では解除されない」ことが判明した場合は、本 issue の範囲をテスト追加からバグ報告 (別 issue) に切り替え、解除実装を `VTSessionSetProperty(session, kVTCompressionPropertyKey_DataRateLimits, NULL)` 方式へ変更する対応を起票する。

## 完了条件

- 「上限あり → 空 Vec で解除 → 出力レートが上限超えに戻る」を実測で検証するテストが `tests/test_encoder.rs` に追加されている
- H.264 で 1 本あれば十分 (既存の cap テストが H.264 / H.265 両方をカバーしているため、解除は方式の検証として 1 コーデックでよい)
- `cargo test --all` が通る
