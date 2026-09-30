# コーデックケーパビリティクエリを拡張する

- Created: 2026-09-30
- Completed: {YYYY-MM-DD}
- Branch: feature/add-codec-info-caps
- Polished: {YYYY-MM-DD}

## 目的

`supported_codecs()` が返す情報が「コーデックごとの可否」と「プロファイル一覧」に留まっており、利用側が「この解像度・この設定でエンコード / デコードできるか」を事前に判定できない。判定できないため、実際にセッションを作成して失敗させるまで分からず、対応可否の判定とエラー処理が利用側に漏れている。

もう一方のバックエンドである nvcodec-rs は `supported_codecs()` が返す `EncodingInfo` / `DecodingInfo` にハードウェアアクセラレーション可否、サポート機能フラグ、解像度の上下限を含めており、利用側がバックエンド非依存で事前判定できる。本クレートにも同等の情報を追加し、4 バックエンドで語彙を揃える。

## 現状

- `src/codec_info.rs` の `EncodingInfo` は `supported` / `hardware_accelerated` / `supports_frame_reordering` / `supports_multi_pass` / `profiles` のみ
- `src/codec_info.rs` の `DecodingInfo` は `supported` / `hardware_accelerated` の 2 つの bool のみで、どちらも `VTIsHardwareDecodeSupported` の結果であり同じ値になる
- エンコーダーの選択結果（どのエンコーダーが使われるか）は公開されておらず、`VTCopySupportedPropertyDictionaryForEncoder` の `encoderIDOut` は現在 `null` を渡して読み捨てている
- `kVTVideoEncoderList_PerformanceRating` / `kVTVideoEncoderList_QualityRating` / `kVTVideoEncoderList_InstanceLimit` / `kVTVideoEncoderList_EncoderName` は `VTCopyVideoEncoderList` の結果として取得できるのに読んでいない
- サポートプロパティ辞書は `kVTCompressionPropertyKey_ProfileLevel` の `kVTPropertySupportedValueListKey` しか読んでおらず、`kVTPropertySupportedValueMinimumKey` / `kVTPropertySupportedValueMaximumKey` / `kVTPropertyTypeKey` / `kVTPropertyReadWriteStatusKey` は使っていない

## 設計方針

### エンコード情報

`EncodingInfo` に、Video Toolbox から実際に取得できる値を追加する。

- エンコーダー識別: `VTCopySupportedPropertyDictionaryForEncoder` の `encoderIDOut` から得られるエンコーダー ID と、`VTCopyVideoEncoderList` の `kVTVideoEncoderList_EncoderName` / `kVTVideoEncoderList_CodecName` から得られる表示名
- 性能指標: `kVTVideoEncoderList_PerformanceRating` / `kVTVideoEncoderList_QualityRating`（いずれも optional なので `Option` で表現する）
- リソース制約: `kVTVideoEncoderList_InstanceLimit`（グローバルなインスタンス上限があるか）
- 設定値の範囲: `VTCopySupportedPropertyDictionaryForEncoder` が返す辞書から、ビットレート・フレームレート・キーフレーム間隔などの数値プロパティの上下限（`kVTPropertySupportedValueMinimumKey` / `kVTPropertySupportedValueMaximumKey`）を読む

`profiles` の取得は既存の `query_encoding_profiles` を使い続ける。

### デコード情報

`DecodingInfo` に、Video Toolbox から実際に取得できる値を追加する。

- コーデックごとのハードウェアデコード可否を判定するヘルパー（`VTIsHardwareDecodeSupported` の薄いラッパー）を追加し、`supported` がハードウェア可否を意味することを rustdoc と型で明確にする
- デコンプレッションセッションのサポートプロパティ辞書（`VTSessionCopySupportedPropertyDictionary`）から、そのコーデックで実際に設定可能な項目を取得する。`supported_codecs()` の中で一時セッションを作る必要があるため、失敗時は「不明」として扱い `supported_codecs()` 全体を失敗させない

### 解像度の上下限の扱い

nvcodec-rs の `EncodingInfo` / `DecodingInfo` は最大・最小解像度を持つが、Video Toolbox には「サポートされる最大解像度」を直接返す API が無い。`VTCopySupportedPropertyDictionaryForEncoder` は引数で受け取った幅・高さに対する辞書を返すだけで、上限を教えてくれない。

そのため本 issue では、解像度の上下限フィールドを追加するかどうかを実装時に確定する。次のいずれかを選び、根拠をコメントに残すこと。

- 候補解像度で `VTCopySupportedPropertyDictionaryForEncoder` を呼び、失敗する解像度を探索して上限を求める（呼び出し回数と所要時間を実測して判断する）
- 上限が信頼できる形で取得できない場合はフィールドを追加せず、取得できない理由を rustdoc に明記する

**取得できない値を 0 や false で埋めたフィールドを追加してはならない。** 利用側が「0 だから非対応」と誤読するため。

### 既存 API との整合

`EncodingInfo` / `DecodingInfo` / `CodecInfo` は `#[non_exhaustive]` ではないため、フィールド追加は破壊的変更になる。CHANGES.md の `[CHANGE]` に、利用側で構造体リテラルを構築している場合は修正が必要である旨を書くこと。`CodecInfo` は `supported_codecs()` の戻り値としてのみ使われるため、構築は crate 内に閉じている。

## 完了条件

- 追加した各フィールドについて、値を取得する根拠となる Video Toolbox API が rustdoc またはコメントに明記されている
- 取得できない値のフィールドを追加していない（追加する場合は実測に基づく根拠がある）
- 追加したフィールドが実環境で妥当な値を返すことをテストで検証する（環境依存の値は「偽でないこと」「取得失敗時に panic しないこと」の範囲で検証する）
- README の「コーデック情報の取得」節を更新する
- CHANGES.md に `[ADD]` と `[CHANGE]` のエントリを追加する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 解決方法

未着手。
