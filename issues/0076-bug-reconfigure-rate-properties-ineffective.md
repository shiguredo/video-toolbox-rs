# reconfigure の data_rate_limits 更新がエンコード開始後に Video Toolbox へ実効しない

- Priority: Medium
- Created: 2026-07-16
- Updated: 2026-07-21
- Completed:
- Model: Fable 5
- Branch: feature/fix-reconfigure-rate-properties
- Polished: 2026-07-31

## 目的

`Encoder::reconfigure` は公開 API として「動的に変更可能な項目は average_bitrate / expected_frame_rate / data_rate_limits の 3 項目」と契約している（`ReconfigureParams` の rustdoc、CHANGES.md の `[CHANGE]` エントリ）。しかし実機での実測により、**エンコード開始後に呼んだ reconfigure の data_rate_limits 更新（設定・解除）が Video Toolbox に実効しない**ことが確認された。`VTSessionSetProperties` は noErr を返すが、出力レートは一切変化しない。なお `average_bitrate` の更新については実測では判定不能（後述）であり、実効しないことは確定していない。

特に `ReconfigureParams::data_rate_limits` の「`Some(空 Vec)` は設定済みの上限を解除する」という rustdoc の契約は、エンコード開始後は実挙動と乖離している（解除しても上限が効き続ける）。利用者（suzume 等）が解除・帯域変更を前提に帯域制御を組むと、気付かないうちに意図した帯域制御が機能しない状態になる。

## 優先度根拠

- 公開 API の契約（rustdoc / CHANGES.md / README）と実挙動が乖離しており、利用者が解除・帯域変更を前提に帯域制御を組むと気付かない不具合になる
- エンコード開始前の reconfigure は機能するため、影響範囲は「エンコード中の動的変更」に限定される
- 即座のクラッシュではないため Medium

## 現状

### 実測結果（2026-07-31、Apple M1 Max / macOS 26.4 / H.264・H.265）

合成フレーム（グラデーション + 下部 1/4 ノイズ帯、960x480、30 fps、`real_time=true`、2 Mbps `average_bitrate`、750 kbps 上限 = 1 秒ウィンドウ 93,750 バイト）をエンコードし、ウィンドウあたりの出力バイト数を上限比で集計した結果:

| シナリオ | 結果 |
|---|---|
| `Encoder::new` で上限設定 | ウィンドウ合計が 0.70x に抑えられる（上限が効く） |
| エンコード開始前に `reconfigure(data_rate_limits: Some([...]))` で上限設定 | **0.70x に抑えられる（上限が効く）** |
| エンコード途中に `reconfigure(data_rate_limits: Some([...]))` で上限設定 | **無制限時と同一の 2.78x のまま**（設定が効かない） |
| 上限設定後にエンコード途中で `reconfigure(data_rate_limits: Some(空 Vec))` で解除 | **解除後も 0.70x に張り付いたまま**（20 秒間観測。解除なしの実行と出力パターンが完全一致） |
| `reconfigure(average_bitrate: Some(300_000))` で変更 | **2.78x のまま**（ただし `Encoder::new` 時に 300k を設定しても 2.78x であり、このノイズ帯コンテンツでは average_bitrate はソフトターゲットとして最低品質に張り付く。実効しないことの判定は不能） |
| `VTSessionSetProperty(session, kVTCompressionPropertyKey_DataRateLimits, NULL)`（解除の代替候補） | **kVTParameterErr (-12902)** |
| `VTSessionCopyProperty` での読み戻し | 常に空配列（設定値が観測できない） |

つまり **エンコード開始前の reconfigure は機能するが、エンコード開始後の reconfigure のレート関連更新は黙殺される**。なお、`expected_frame_rate` の更新が Video Toolbox 側で実効するかは未検証（`next_input_pts` の再スケールなど config レベルの更新は確認済み）。

### 再現手順

1. 合成フレーム（グラデーション + 下部 1/4 ノイズ帯、960x480）を 30 fps の PTS でエンコードする（`real_time=true`、`average_bitrate=2_000_000`、H.264）
2. 構築時に `data_rate_limits = Some([93_750 バイト / 1 秒])` を設定し、120 フレーム（4 秒ぶん）エンコードして上限が効いていることを確認する
3. `reconfigure(ReconfigureParams { data_rate_limits: Some(Vec::new()), .. })` で解除する（`Ok` が返り、`config().data_rate_limits` は `None` になる）
4. さらに 600 フレーム（20 秒ぶん）エンコードし、ウィンドウ合計が上限を超えて増えるか確認する → **増えない（0.70x のまま）**

上限設定（エンコード途中）の再現も同様に、上限なしで構築してエンコード途中に `reconfigure(data_rate_limits: Some([...]))` を呼び、出力が 2.78x のまま（無制限時と同一）であることを確認する。

## 設計方針

### 原因の切り分け（実測済みの項目と未実施の項目）

実測済み:

- **設定タイミングの影響**: エンコード開始前の reconfigure は機能し（0.70x）、エンコード開始後は機能しない（2.78x）。つまり `VTSessionSetProperties` 自体は機能しており、**エンコード開始後のプロパティ変更が Video Toolbox に反映されない**ことが原因の中心
- **プロパティの観測**: `VTSessionCopyProperty` で設定値が読み戻せない（常に空配列）

未実施の切り分け:

- **real_time の影響**: `real_time=false` のセッションで同じ実験を行い、実効しない原因が real_time セッションの制約かを確認する
- **Apple の仕様確認**: VTCompressionProperties.h の「By default, no data rate limits are set.」「some codecs do not support limiting to specified data rates」等の記述と照合する

### 修正方針

エンコード開始後のレート変更が Video Toolbox の制約で反映されない場合、`reconfigure` の「動的更新」契約と矛盾するため、以下のいずれかの設計判断を行う:

- **実効化する**: `reconfigure` のレート関連更新をセッション再作成ベースに変更する（解像度・コーデック変更と同じく `Encoder` の作り直しに統一する）
- **契約を実態に合わせる**: `reconfigure` をエンコード開始前のみ有効な API として契約を再定義し、rustdoc / CHANGES.md / README を修正する（エンコード開始後の更新は呼び出し側に `Encoder` 再生成を促す）

実効化の回帰テストは「設定・解除が出力レートに実際に反映される」ことを実測で検証する（既存の `data_rate_limits_cap_windowed_output` のハーネスを拡張する形。`tests/test_encoder.rs`）。なお、実効化する場合のセッション再作成は `create_compression_session` を経由するため、issue 0058（`create_compression_session` で `VTSessionSetProperties` 失敗時にセッションがリークする）を先に修正しておく必要がある。

## 完了条件

- 実効しない原因が特定されている（実測済みの切り分け + 未実施の切り分け項目の結果）
- 「実効化する」を採った場合: 修正が実施され、実測で「設定・解除が出力レートに反映される」ことを検証するテスト（`data_rate_limits_cap_windowed_output` の拡張等）が追加されている
- 「契約を実態に合わせる」を採った場合: `ReconfigureParams` の rustdoc / CHANGES.md / README の契約を実態に合わせて修正している（エンコード開始前のみ有効であることを明記）
- `CHANGES.md` にエントリを追記する
- `cargo test --workspace` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo fmt --all -- --check` が通る
