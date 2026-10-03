# コーデックケーパビリティクエリを拡張する

- Created: 2026-09-30
- Completed: 2026-10-03
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

設計方針では `EncodingInfo` にエンコーダーの識別情報・指標・インスタンス上限を追加する予定だったが、これらはすべて「どのエンコーダーが選ばれるか」に依存し、その選択結果は解像度によって変わる。1920x1080 のような代表解像度での照会結果を `EncodingInfo` に持たせると、実際にエンコードする条件と一致しない値を返すことになる。そのため、解像度に依存しない情報と、解像度を指定して照会する情報を分けて公開する形に変更した。

### 解像度に依存しない API

- `supported_codecs()`: コーデック単位の情報。`VTCopyVideoEncoderList` の結果を使うだけで、解像度に依存する情報は含まない
- `CodecInfo`: `codec` / `decoding` / `encoders`。`encoders` はエンコーダー 1 件ずつの一覧で、空ならエンコード非対応
- `EncodingInfo`: エンコーダー 1 件の情報。`encoder_id` / `encoder_name` / `codec_name` / `hardware_accelerated` / `supports_frame_reordering` / `supports_multi_pass` / `performance_rating` / `quality_rating` / `has_instance_limit` を持つ
- `DecodingInfo`: `hardware_accelerated` のみ。`VTIsHardwareDecodeSupported` の結果で、解像度に依存しない

`DecodingInfo` には `supported` と `hardware_accelerated` の 2 つの bool があったが、どちらも `VTIsHardwareDecodeSupported` の結果で常に同じ値になり、`supported` は名前から「デコードできるか」と誤読される。削除して `hardware_accelerated` に一本化した。ソフトウェアデコードを含めたデコード可否は Video Toolbox から事前に取得できない。`VTRegisterSupplementalVideoDecoderIfAvailable` で追加のデコーダーを登録すると `VTIsHardwareDecodeSupported` の結果が変わりうるためである (macOS 26.5.2 / M1 では、VP9 はこの関数を呼ぶ前は `false`、呼んだ後は `true` になった)。この点は `DecodingInfo` の rustdoc に明記した。

`EncodingInfo` の `hardware_accelerated` / `supports_frame_reordering` / `supports_multi_pass` はエントリ自身の属性であり、解像度に依存しない。設計方針にあった「対象コーデックのエンコーダーを集計した 1 つの値」ではなく、エンコーダー 1 件ずつの値として返す。集計値は Video Toolbox が返す値ではなく `encoders` を畳み込めば利用側で求められるためである。そのため集計用の `supported` は持たせず、エンコード対応かどうかは `encoders` が空かどうかで判定する。エンコーダー一覧に同じ粒度の型を 2 つ (`EncodingInfo` と `VideoEncoderInfo`) 用意する必要は無いため、`VideoEncoderInfo` は作らず `EncodingInfo` に統合した。

### 解像度を指定する API

- `query_encoding_capabilities(codec, width, height) -> Option<EncodingCapabilities>`
- `EncodingCapabilities { encoder: EncodingInfo, profiles: Option<EncodingProfiles> }`

`encoder` はその解像度で選ばれるエンコーダーで、`CodecInfo::encoders` のいずれかの要素になる。この解像度でハードウェアエンコーダーが使えるかは `encoder.hardware_accelerated` で判定する。`profiles` はそのエンコーダーが扱うプロファイルで、Video Toolbox がプロファイル一覧を返さなかった場合は `None` になる。`EncodingProfiles` には「プロファイル情報なし」を表す `None` バリアントがあったが、プロファイル情報を取得できなかったことは `Option<EncodingProfiles>` で表せるため削除した (`Some(H264(vec![]))` の「取得できて 0 件」と区別できる)。エンコーダーを特定できない場合は `EncodingCapabilities` ではなく `None` を返す。取得できなかった値を `false` や `0` で埋めた構造体を返さないためである。

### 解像度ごとのハードウェアエンコード可否の判定方法

`encoderSpecification` に NULL を渡した `VTCopySupportedPropertyDictionaryForEncoder` が返す `encoderIDOut` を `VTCopyVideoEncoderList` と突き合わせると、その解像度で選ばれるエンコーダーが分かる。ハードウェアエンコーダーが使えない解像度では Video Toolbox がソフトウェアエンコーダーを選ぶため、突き合わせたエントリの `kVTVideoEncoderList_IsHardwareAccelerated` で可否を判定できる。macOS 26.5.2 / M1 での実測では、H.264 の 1920x1080 で `com.apple.videotoolbox.videoencoder.ave.avc` (ハードウェア)、16384x16384 で `com.apple.videotoolbox.videoencoder.h264` (ソフトウェア) が返った。

`kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder` を `kCFBooleanTrue` にして呼ぶ方法でも判定できる。この指定を付けた照会は、ハードウェアエンコーダーが使えない解像度では -12903 で失敗する。ただし H.264 / HEVC の 14 種類の解像度 (1920x1080 / 3840x2160 / 4096x4096 / 4096x4097 / 5120x2880 / 8192x8192 / 8193x8193 / 16384x16384 と HEVC の 8192x16384 / 65536x8192 など) で両者を比較したところ、要求付き照会の成否と NULL 指定で選ばれたエントリの `hardware_accelerated` はすべて一致した。1 回の照会で判定できる NULL 指定だけを使い、2 回目の照会と encoderSpecification の組み立ては行わない形にした。NULL 指定の照会は `VTCompressionSessionCreate` と同じ既定の選択なので、返る `encoder` は実際に使われるエンコーダーになる。

この照会が成功しても、ハードウェアエンコーダーの資源が枯渇している場合にセッションの生成が失敗することがある。この照会は現在の資源の空き状況ではなく、その解像度で選択されるエンコーダーを返すためである。この点は rustdoc に明記した。

### エンコーダー情報の取得

`VTCopyVideoEncoderList` の各エントリから、`kVTVideoEncoderList_EncoderID` / `kVTVideoEncoderList_EncoderName` / `kVTVideoEncoderList_CodecName` / `kVTVideoEncoderList_IsHardwareAccelerated` / `kVTVideoEncoderList_SupportsFrameReordering` / `kVTVideoEncoderList_SupportsMultiPass` / `kVTVideoEncoderList_PerformanceRating` / `kVTVideoEncoderList_QualityRating` / `kVTVideoEncoderList_InstanceLimit` を読む。`kVTVideoEncoderList_EncoderID` を取得できないエントリはエンコーダーとして識別できないため一覧から除外する。指標は optional なキーのため `Option<f64>`、インスタンス上限はキーが無い場合を「上限なし」と誤読させないため `Option<bool>` とした。

`query_encoding_capabilities()` は、`encoderIDOut` が返す ID と一致するエントリを `CodecInfo::encoders` と同じ経路で探す。一覧からエントリを特定できなかった場合は ID 以外の情報を返せないため `None` を返す。

### フレームリオーダリングの既定値の修正

`kVTVideoEncoderList_SupportsFrameReordering` は、キーが無い場合に true と見なす仕様 (VTVideoEncoderList.h) だが、キーの有無を区別せずに `false` として扱っていたため `supports_frame_reordering` が常に `false` になっていた。`CFBoolean` を取り出す `get_cf_bool` を `Option<bool>` を返すようにし、キーごとに異なる既定値 (フレームリオーダリングは true、インスタンス上限とマルチパスは false) を呼び出し側で適用するように修正した。

### 追加しなかった API

設計方針にあった「コーデックごとのハードウェアデコード可否を判定するヘルパー (`VTIsHardwareDecodeSupported` の薄いラッパー)」は追加していない。`DecodingInfo` は `supported_codecs()` が返す `CodecInfo::decoding` から完全に取得でき、単一コーデック用の照会関数を足しても情報の入手先が重複するだけである。解像度を指定して照会する `query_encoding_capabilities()` と対になる関数でもない (`DecodingInfo` は解像度に依存せず、`VTIsHardwareDecodeSupported` 以上の情報を取得できない)。ハードウェアデコード可否を意味することは `hardware_accelerated` という名前と `DecodingInfo` の rustdoc で明確にした (`supported` は削除した)。

`DecodingInfo` のデコンプレッションセッションのサポートプロパティ辞書は追加していない。`VTSessionCopySupportedPropertyDictionary` を使うには有効な `CMVideoFormatDescription` を持つセッションが必要で、H.264 / HEVC では SPS / PPS などのパラメータセットが要求されるため、`supported_codecs()` の中では用意できない。macOS 26.5.2 / M1 でパラメータセットを与えずに `CMVideoFormatDescriptionCreate` と `VTDecompressionSessionCreate` を呼んだ実測では、H.264 のみ成功し、HEVC は -8971、VP9 / AV1 は -12906 で失敗した。`kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder` を付けても結果は同じで、解像度に依存するデコード可否は取得できない。全コーデックで値を取得できないフィールドは追加しない方針に従い、取得できない理由を `DecodingInfo` の rustdoc に明記した。

解像度の上下限も追加していない。`kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder` を使った二分探索でハードウェアエンコーダーが対応する範囲を実測したところ、H.264 は 1x1〜4096x4096 の矩形だったが、HEVC は「幅 8192 以下かつ高さ 16384 以下」または「幅 65536 以下かつ高さ 8192 以下」の和集合であり、単一の矩形では表せなかった (8193x8193 不可、65536x8192 可、16384x8640 不可、32767x8192 可、10000x10000 不可)。単一の下限・上限を返すと「65536x16384 もエンコードできる」といった誤った判定を招くため、min/max フィールドを追加せず、解像度を指定する `query_encoding_capabilities()` で正確に判定する形にした。理由は `EncodingInfo` の rustdoc に明記した。`VTCopySupportedPropertyDictionaryForEncoder` は幅・高さが正であれば 65535x65535 でも成功する (NULL 指定時は辞書の項目数が減るだけでエラーにならない) ため、この API 単体では上限を探索できない。

ビットレート・フレームレート・キーフレーム間隔などの数値プロパティの上下限も追加していない。`kVTPropertySupportedValueMinimumKey` / `kVTPropertySupportedValueMaximumKey` は、`VTCopySupportedPropertyDictionaryForEncoder` が返す辞書にも、圧縮セッションの `VTSessionCopySupportedPropertyDictionary` が返す辞書にも含まれていなかった (`AverageBitRate` / `ExpectedFrameRate` / `MaxKeyFrameInterval` はいずれも `PropertyType` と `ReadWriteStatus` のみ)。`kVTVideoEncoderList_SupportedSelectionProperties` にも範囲の情報は無かった。`ReadWriteStatus` から「設定可能か」を導くこともできない。ソフトウェアエンコーダーの辞書は該当する項目を空の辞書 (`{}`) で返すが、実測では 7680x4320 の H.264 ソフトウェアエンコーダーに `AverageBitRate` を設定すると `VTSessionSetProperty` は成功 (status 0) した。

### テスト

`tests/test_codec_info.rs` を更新し、コーデックごとにエンコーダー一覧が返ること (H.264 / HEVC はハードウェアとソフトウェアのエントリを含み、VP9 / AV1 は空)、各エントリの `supports_frame_reordering` が true で `supports_multi_pass` が false になること、解像度を指定した照会がエンコーダーの ID・表示名・指標とプロファイルを返すこと、解像度によって結果が変わること (1920x1080 では `encoder.hardware_accelerated` が `true`、16384x16384 では `false` になり、ソフトウェアエンコーダーに切り替わる)、エンコード非対応のコーデックと、幅・高さが 0 または `i32` で表現できない解像度では `None` になることを検証している。選ばれたエンコーダーが `CodecInfo::encoders` の要素であることも固定した。`src/codec_info.rs` には、キーが無い CFBoolean / CFBoolean の true と false / CFBoolean でない値の 3 通りで `get_cf_bool` の既定値の扱いを検証する単体テストと、`kVTVideoEncoderList_CodecType` から FourCC を取り出す `encoder_fourcc` のテストを追加した。

### ドキュメント

README の「コーデック情報の取得」を、解像度に依存しない `supported_codecs()` と解像度を指定する `query_encoding_capabilities()` に分けて書き直した。`skills/shiguredo-video-toolbox/SKILL.md` の API 表と VP9 判定のサンプルも新しい API に合わせた。CHANGES.md には `[CHANGE]` を 2 件 (`CodecInfo::encoders` への変更と `EncodingInfo` の再定義 / `DecodingInfo::supported` の削除と `hardware_accelerated` への一本化)、`[ADD]` (`query_encoding_capabilities` / `EncodingCapabilities`)、`[FIX]` (`supports_frame_reordering` の既定値) のエントリを追加した。
