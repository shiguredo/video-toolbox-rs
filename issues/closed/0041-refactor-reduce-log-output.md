# ログ出力を削減し、エラー情報をユーザーに伝搬する

- Priority: Low
- Created: 2026-05-13
- Updated: 2026-07-21
- Completed: 2026-08-06
- Model: deepseek-v4-pro
- Branch: feature/change-reduce-log-output
- Polished: 2026-07-31

## 方針

ユーザーにエラーの情報を `Result<>` で伝えられる場合、 `tracing::error!()` は不要とする。

## 背景・根拠

現在 `src/encoder.rs`・`src/decoder.rs` 内の複数の関数が `Option` を返しつつ `tracing::error!()` でエラー情報を出力している。これらはログでしか状況を伝える手段がないために存在するが、 `Result<_, Error>` に変更すればエラー情報をコールバック経由でユーザーに伝搬できるため、ログ出力は不要になる。

また `Error` 型の全文字列フィールドが `&'static str` に制約されているため、動的な値（例: 実際の `block_len` の数値、NAL ヘッダ長の実値、プレーン名の動的生成等）をエラーメッセージに含められず、 `tracing::error!()` で補足情報を出力せざるを得ない箇所が多い。フィールド型を `String` に一括変更し、 `format!()` による動的なエラーメッセージ構築を可能にする。

現状 `tracing::error!()` は全 17 箇所（`src/encoder.rs` 13 箇所、`src/decoder.rs` 3 箇所、`src/types.rs` 1 箇所）に存在する。なお `src/encoder.rs` の `Drop` 実装には `tracing::error!()` は含まれていない。

issue 0042（エラー時のユーザーデータ付与検討）は本 issue と関連するが独立した検討事項であり、本 issue の範囲外とする。本 issue が新設するエラー伝搬は `Err(Error)` の形のままであり、0042 の設計判断（エラー時にユーザーデータを付与するか）には依存しない。

本 issue の前提として、 issue 0039（trait ベースのハンドラ）および issue 0040（ハンドラの型パラメータ変更）が適用済みのコードベースを想定する。なお issue 0053（`src/encoder.rs` のモジュール分割）も `process_encoded_output` / `extract_h264_params` / `extract_h265_params` / `vec_u8_from_raw_parts_safe` 等の同じ関数群を対象とする。先に 0053 を実施する場合は、本 issue の変更対象は分割後のモジュールに移る。

## CHANGES.md 追記方針

本変更は Error 型の公開フィールド型変更を含む破壊的変更であるため、 `[CHANGE]` として以下の 1 エントリを `CHANGES.md` の `## develop` セクションに追記する:

- `[CHANGE]` `Error` 型の全 `&'static str` フィールドを `String` に変更し、エラーメッセージに動的な値を含められるようにする
  - `Option` を返していた内部関数を `Result<_, Error>` に変更し、エラー情報をコールバック経由でユーザーに伝搬する
  - 不要になった `tracing::error!()` を削除する
  - @melpon

## 対応内容

### 1. Error 型の全 `&'static str` フィールドを `String` に変更する

現状の `Error` 型（`src/error.rs` の `Error` enum）では以下の全 7 フィールドが `&'static str` に制約されている:

- `VideoToolbox.function`
- `InsufficientFrameData.plane`
- `UnsupportedCodec.codec`
- `InvalidConfig.field`, `InvalidConfig.reason`
- `LimitExceeded.reason`
- `CfObjectCreationFailed.function`

なお `UnknownPixelFormat` バリアント（`expected: PixelFormat`, `fourcc: u32`）は文字列フィールドを持たないため本変更の対象外である。

フィールド型を一律 `&'static str` のままにしておくと、今後動的な値を含めたくなったときに都度破壊的変更が必要になる。`reason` 系以外のフィールド（`field` / `plane` / `codec` / `function`）は現状すべて静的リテラルで構築されているが、部分変更にすると将来また破壊的変更が必要になるため、すべて `String` に一括変更する。

```rust
// src/error.rs 変更後
pub enum Error {
    VideoToolbox {
        status: i32,
        function: String,              // &'static str → String
    },
    PixelFormatMismatch {
        expected: PixelFormat,
        actual: PixelFormat,
    },
    InsufficientFrameData {
        plane: String,                 // &'static str → String
        expected: usize,
        actual: usize,
    },
    UnsupportedCodec {
        codec: String,                 // &'static str → String
    },
    InvalidConfig {
        field: String,                 // &'static str → String
        reason: String,                // &'static str → String
    },
    LimitExceeded {
        reason: String,                // &'static str → String
    },
    CfObjectCreationFailed {
        function: String,              // &'static str → String
    },
}
```

これにより `format!()` で動的な値を含むエラーメッセージを構築できるようになり、 `tracing::error!()` で補足情報を出力する必要がなくなる。

#### Error::check() のシグネチャ変更

`Error::check()` は `function: &'static str` を受け取り `Error::VideoToolbox { function }` を生成している。 `VideoToolbox.function` を `String` に変更するのに合わせ、パラメータ型を `function: impl Into<String>` に変更する:

```rust
// 変更前
pub(crate) fn check(status: i32, function: &'static str) -> Result<(), Self> {

// 変更後
pub(crate) fn check(status: i32, function: impl Into<String>) -> Result<(), Self> {
    // ...
    Err(Self::VideoToolbox { status, function: function.into() })
}
```

`&'static str` は `Into<String>` を実装しているため、既存の全呼び出し側（`Error::check(status, "関数名")`）の変更は不要。

#### Display 実装への影響

`Display` 実装は全フィールドを `write!(f, ...)` で表示しており、 `&'static str` も `String` も `Display` を実装しているため、型変更後もコード修正は不要。

#### API 互換性

`Error` は `pub use error::Error` で公開されている。全 `&'static str` フィールドの `String` 変更は破壊的変更であり、 `[CHANGE]` として扱う。ユーザーが `Error::InvalidConfig { field: "fps", reason: "zero" }` のように構築・パターンマッチしているコードでは、各リテラルを `.to_string()` に変更するか、 match guard で比較する必要がある。

#### 構築箇所の対応

`String` 化に伴い、`Error` を構築している全箇所で `.into()` または `.to_string()` が必要になる。対象は以下の関数群:

- `src/types.rs`: `validate_video_dimensions_for_toolbox`（`InvalidConfig` 4 箇所）、 `cf_dictionary` / `cf_array` / `cf_number_i32` / `cf_number_i64` / `cf_number_f64`（各 `CfObjectCreationFailed` 1 箇所）
- `src/encoder.rs`: `validate_average_bitrate` / `validate_fps_numerator` / `validate_expected_frame_rate`（各 2 箇所）、 `validate_data_rate_limits`（4 箇所）、 `validate_config`（`fps_denominator` / `max_key_frame_interval` / `max_frame_delay_count` の 3 箇所）、 `copy_plane`（10 箇所）、 `validate_frame_data`（5 箇所）、 `frame_byte_len_checked`（1 箇所）、 `Encoder::reconfigure`（1 箇所）、 `process_encoded_output`（5 箇所）、 `encode` / `encode_pixel_buffer`（各 1 箇所）
- `src/decoder.rs`: `create_format_description`（`nalu_len_bytes` / `parameter_sets` の `InvalidConfig` 4 箇所）、 `wrap_unsupported_codec_error`（2 箇所）、 `output_callback`（1 箇所）

行番号で列挙しないのは、コード変更で即ずれるため。いずれもコンパイルエラーとして機械的に検出されるため、実装時はコンパイラの指示に従って対応すればよい。

### 2. `Option` を返している関数を `Result<_, Error>` に変更する

現状 `Option` を返しつつ `tracing::error!()` でエラー内容を出力している関数を `Result` に変更し、エラー情報を呼び出し元に伝搬する。

#### 2.1 `vec_u8_from_raw_parts_safe`（`src/encoder.rs` のフリー関数）

現在の戻り値: `Option<Vec<u8>>`

| エラーパス | 使用する Error バリアント |
|---|---|
| `len > MAX_PARAMETER_SET_COPY_BYTES` | `LimitExceeded { reason: format!("{context}: parameter set length {len} exceeds defensive maximum {max}", max = MAX_PARAMETER_SET_COPY_BYTES) }` |
| `ptr.is_null() && len > 0` | `LimitExceeded { reason: format!("{context}: null pointer with non-zero length") }` |

#### 2.2 `extract_h264_params`（`src/encoder.rs` の `Encoder` メソッド）

現在の戻り値: `Option<ParameterSets>`

| エラーパス | 使用する Error バリアント |
|---|---|
| description.is_null() | `LimitExceeded { reason: "CMVideoFormatDescription is null in extract_h264_params".into() }` |
| CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (1 回目) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetH264ParameterSetAtIndex".into() }` |
| nalu_header_length != 4 | `LimitExceeded { reason: format!("unexpected NAL unit header length: {nalu_header_length}") }` |
| CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (2 回目以降) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetH264ParameterSetAtIndex".into() }` |
| vec_u8_from_raw_parts_safe 失敗 | 伝搬（`?` 演算子） |

関数ポインタ型の変更: `process_encoded_output` の引数 `extract_params` の型を `unsafe fn(sys::CMVideoFormatDescriptionRef) -> Option<ParameterSets>` から `unsafe fn(sys::CMVideoFormatDescriptionRef) -> Result<ParameterSets, Error>` に変更する。`Result` 化に伴い、`process_encoded_output` 内の `extract_params` 失敗時の汎用エラー（`"failed to extract codec parameter sets"`）は消滅し、抽出関数の詳細エラーがそのままハンドラ経由で伝搬される。

#### 2.3 `extract_h265_params`（`src/encoder.rs` の `Encoder` メソッド）

現在の戻り値: `Option<ParameterSets>`

| エラーパス | 使用する Error バリアント |
|---|---|
| description.is_null() | `LimitExceeded { reason: "CMVideoFormatDescription is null in extract_h265_params".into() }` |
| CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (1 回目) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex".into() }` |
| nalu_header_length != 4 | `LimitExceeded { reason: format!("unexpected NAL unit header length: {nalu_header_length}") }` |
| CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (2 回目以降) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex".into() }` |
| vec_u8_from_raw_parts_safe 失敗 | 伝搬（`?` 演算子） |

#### 2.4 `take_user_data`（`src/encoder.rs` の `Encoder` メソッド）

現在の戻り値: `Option<H::UserData>`

| エラーパス | 使用する Error バリアント |
|---|---|
| source_frame_ref_con.is_null() | `LimitExceeded { reason: format!("{callback_name}: source_frame_ref_con is null") }` |

#### 2.5 `take_pending_decode`（`src/decoder.rs` の `Decoder` メソッド）

現在の戻り値: `Option<Box<PendingDecode<H::UserData>>>`

| エラーパス | 使用する Error バリアント |
|---|---|
| source_frame_ref_con.is_null() | `LimitExceeded { reason: format!("{callback_name}: source_frame_ref_con is null") }` |

注: null ポインタ系のエラーに `LimitExceeded` を使うのは、`copy_plane` の null チェック（例: `"CVPixelBuffer base address for plane is null"`）と同じ既存の慣行に従う。合わせて `LimitExceeded` の rustdoc の文言を「内部カウンタや算術の上限超過」から「防御的な上限超過や異常値（null ポインタ等）」に更新する。

### 3. FFI コールバックの処理順序を変更する

現在の FFI コールバックは `take_user_data`/`take_pending_decode` → `callback_from_ref_con` の順で呼んでいるため、 `take_user_data`/`take_pending_decode` が失敗した時点でハンドラが未取得でありエラーを伝搬できない。

処理順序を `callback_from_ref_con` → `take_user_data`/`take_pending_decode` に変更し、 `take_user_data`/`take_pending_decode` の失敗をハンドラ経由でユーザーに伝搬できるようにする。

対象箇所:
- `src/encoder.rs`: `process_encoded_output`
- `src/decoder.rs`: `output_callback`

#### 安全性の検証

2 つのポインタ（`source_frame_ref_con` と `output_callback_ref_con`）は独立しており、 `Box::from_raw` と `&mut *ptr` のライフタイム競合は発生しない。コールバックの実行は既存実装と同じく Video Toolbox のセッション単位の直列化に依存しており、本変更で新たな並行アクセスを導入しない。

`callback_from_ref_con` が null を返す場合でも、`take_user_data` / `take_pending_decode` は handler の有無にかかわらず後で必ず呼ばれるため、`source_frame_ref_con` の `Box` は常に回収される（リークしない）。take 系の失敗時は、ハンドラが取得できていればエラーを通知し、取得できていなければ通知なしで終了する。

#### encoder.rs `process_encoded_output` 変更後コード例

```rust
unsafe fn process_encoded_output(
    output_callback_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    sample_buffer: sys::CMSampleBufferRef,
    status: i32,
    callback_name: &'static str,
    extract_params: unsafe fn(sys::CMVideoFormatDescriptionRef) -> Result<ParameterSets, Error>,
) {
    let handler = unsafe { Self::callback_from_ref_con(output_callback_ref_con, callback_name) };

    let user_data = match unsafe { Self::take_user_data(source_frame_ref_con, callback_name) } {
        Ok(data) => data,
        Err(e) => {
            if let Some(h) = handler {
                Self::invoke_callback(h, Err(e.into()));
            }
            return;
        }
    };

    let Some(handler) = handler else {
        // callback_from_ref_con が null。source_frame_ref_con は take_user_data 内で消費済み。
        // ハンドラ不在のため以降の処理は不可。
        return;
    };

    if let Err(e) = Error::check(status, callback_name) {
        Self::invoke_callback(handler, Err(e.into()));
        return;
    }

    // フレームドロップ等で sample_buffer が NULL になる場合がある
    if sample_buffer.is_null() {
        let e = Error::LimitExceeded {
            reason: "encoded sample buffer is null".into(),
        };
        Self::invoke_callback(handler, Err(e.into()));
        return;
    }

    unsafe {
        let data_buffer = sys::CMSampleBufferGetDataBuffer(sample_buffer);
        if data_buffer.is_null() {
            let e = Error::LimitExceeded {
                reason: "CMSampleBufferGetDataBuffer returned null".into(),
            };
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        let block_len = sys::CMBlockBufferGetDataLength(data_buffer);
        if block_len > MAX_ENCODED_BLOCK_COPY_BYTES {
            let e = Error::LimitExceeded {
                reason: format!(
                    "CMBlockBufferGetDataLength {block_len} exceeds defensive maximum {max}",
                    max = MAX_ENCODED_BLOCK_COPY_BYTES,
                ),
            };
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }
        let mut data = vec![0u8; block_len];
        let status = sys::CMBlockBufferCopyDataBytes(
            data_buffer,
            0,
            block_len,
            data.as_mut_ptr().cast(),
        );
        if let Err(e) = Error::check(status, "CMBlockBufferCopyDataBytes") {
            Self::invoke_callback(handler, Err(e.into()));
            return;
        }

        let description = sys::CMSampleBufferGetFormatDescription(sample_buffer);
        let keyframe = is_keyframe(sample_buffer);

        let (vps_list, sps_list, pps_list) = if keyframe {
            if description.is_null() {
                let e = Error::LimitExceeded {
                    reason: "CMSampleBufferGetFormatDescription returned null for keyframe".into(),
                };
                Self::invoke_callback(handler, Err(e.into()));
                return;
            }
            match extract_params(description) {
                Ok(params) => params,
                Err(e) => {
                    Self::invoke_callback(handler, Err(e.into()));
                    return;
                }
            }
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        let frame = EncodedFrame {
            keyframe,
            sps_list,
            pps_list,
            vps_list,
            data,
            user_data,
        };
        Self::invoke_callback(handler, Ok(frame));
    }
}
```

#### decoder.rs `output_callback` 変更後コード例

```rust
unsafe extern "C" fn output_callback(
    decompression_output_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: sys::VTDecodeInfoFlags,
    image_buffer: sys::CVImageBufferRef,
    _presentation_time_stamp: sys::CMTime,
    _presentation_duration: sys::CMTime,
) {
    let callback_name = "output_callback";
    let handler = unsafe { Self::callback_from_ref_con(decompression_output_ref_con, callback_name) };

    let pending = match unsafe { Self::take_pending_decode(source_frame_ref_con, callback_name) } {
        Ok(p) => p,
        Err(e) => {
            if let Some(h) = handler {
                Self::invoke_callback(h, Err(e.into()));
            }
            return;
        }
    };

    let Some(handler) = handler else {
        // callback_from_ref_con が null。source_frame_ref_con は take_pending_decode 内で消費済み。
        return;
    };

    let PendingDecode {
        user_data,
        pixel_format,
        ..
    } = *pending;

    // ... 以下、既存のエラー通知とフレーム構築ロジック（変更なし）
}
```

### 4. 上記変更に伴い不要になった `tracing::error!()` を削除する

以下の 13 箇所（`src/encoder.rs` 12 箇所、`src/decoder.rs` 1 箇所）の `tracing::error!()` を削除する:

| ファイル | 関数・エラーパス | 理由 |
|---|---|---|
| src/encoder.rs | `take_user_data` の source_frame_ref_con null チェック | `Result` 化でハンドラ経由伝搬 |
| src/encoder.rs | `process_encoded_output` の block_len 超過 | `LimitExceeded.reason` に動的値を含めて伝搬 |
| src/encoder.rs | `extract_h264_params` の description null チェック | `Result` 化で伝搬 |
| src/encoder.rs | `extract_h264_params` の CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (1 回目) | 同上 |
| src/encoder.rs | `extract_h264_params` の nalu_header_length 検証 | 同上 |
| src/encoder.rs | `extract_h264_params` の CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (2 回目) | 同上 |
| src/encoder.rs | `extract_h265_params` の description null チェック | `Result` 化で伝搬 |
| src/encoder.rs | `extract_h265_params` の CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (1 回目) | 同上 |
| src/encoder.rs | `extract_h265_params` の nalu_header_length 検証 | 同上 |
| src/encoder.rs | `extract_h265_params` の CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (2 回目以降) | 同上 |
| src/encoder.rs | `vec_u8_from_raw_parts_safe` の len 超過 | `Result` 化で伝搬 |
| src/encoder.rs | `vec_u8_from_raw_parts_safe` の null ポインタ | 同上 |
| src/decoder.rs | `take_pending_decode` の source_frame_ref_con null チェック | `Result` 化で伝搬（順序入れ替え後） |

## 対象外

以下はエラーの伝搬先が存在しないため、ログ出力を残す:

- `callback_from_ref_con` の null チェック（`src/encoder.rs` / `src/decoder.rs` の同名関数）: ハンドラポインタ自体が null であり伝搬先がない
- Drop 実装内のエラー:
  - `Decoder::drop` 内の `self.finish()` 失敗（`src/decoder.rs`）
  - `CvPixelBufferUnlockGuard::drop` 内のアンロック失敗（`src/types.rs`）

エンコーダー側の `Drop` 実装（`src/encoder.rs`）は `VTCompressionSessionInvalidate` + `CFRelease` のみで `tracing::error!()` を含んでおらず、対応不要。

## テスト戦略

### 単体テスト

`Error` の `String` フィールド化に伴い、リテラルパターンマッチ（`String` フィールドへの `&str` リテラルのマッチ）はコンパイルエラーになる。`Error::UnsupportedCodec { .. }` のように全フィールドを `..` で無視している箇所は変更不要だが、それ以外のリテラルマッチはすべて match guard に変更する必要がある。修正対象は以下のとおり:

- **`tests/test_encoder.rs`**: 21 箇所
  - `Error::InvalidConfig { field: "..." }` の field リテラル 7 箇所（`encoder_rejects_zero_width` 等）: `field` を `field if field == "..."` の match guard に変更
  - `Error::InvalidConfig { field: "...", reason: "..." }` の field + reason リテラル 11 箇所（`reconfigure_rejects_*` 等）: 両方を match guard に変更
  - `Error::InsufficientFrameData { plane: "Y" / "U" / "UV" }` の plane リテラル 3 箇所: match guard に変更
- **`tests/test_decoder.rs`**: `Error::InvalidConfig { field: "width" / "height" }` の field リテラル 2 箇所を match guard に変更
- **`src/encoder.rs` の `#[cfg(test)] mod tests`**: `reconfigure_overflows_when_rescaled_pts_exceeds_i64_max` の `LimitExceeded { reason: "rescaled presentation timestamp overflow" }` を match guard に変更
- **`tests/test_error.rs`**: `Error::LimitExceeded` / `Error::CfObjectCreationFailed` の構築箇所（2 箇所）に `.into()` を追加

なお、本変更で新設するエラー伝搬パス（`take_user_data` / `take_pending_decode` の null 検出）は、`encode` / `decode` が `Box::into_raw` で非 null を保証しているため公開 API 経由では再現できない。そのためテストは追加せず、防御的コードとして実装のみとする。

### PBT / Fuzzing

本変更は入力検証ロジックの追加を伴わないため、新規 PBT および fuzzing は不要。FFI コールバックの処理順序変更は Video Toolbox の実 FFI に依存し、モック・スタブを使わない方針のため、既存のラウンドトリップテストで回帰を確認する。

## 変更対象ファイル一覧

| ファイル | 変更内容 |
|---|---|
| `src/error.rs` | 全 `&'static str` フィールドを `String` に変更、 `Error::check()` のパラメータ型を `impl Into<String>` に変更、 `LimitExceeded` の rustdoc を更新 |
| `src/encoder.rs` | `take_user_data` / `extract_h264_params` / `extract_h265_params` / `vec_u8_from_raw_parts_safe` を `Result` 化、関数ポインタ型変更、 `process_encoded_output` の処理順序入れ替え、全構築箇所の `.into()` 追加、 `tracing::error!()` 12 箇所削除、内蔵テストの match guard 修正 |
| `src/decoder.rs` | `take_pending_decode` を `Result` 化、 `output_callback` の処理順序入れ替え、全構築箇所の `.into()` 追加、 `tracing::error!()` 1 箇所削除 |
| `src/types.rs` | `Error` 構築箇所の `.into()` 追加 |
| `tests/test_encoder.rs` | リテラルパターンマッチ 21 箇所を match guard に変更 |
| `tests/test_decoder.rs` | リテラルパターンマッチ 2 箇所を match guard に変更 |
| `tests/test_error.rs` | `Error` 構築箇所の `.into()` 追加 |
| `CHANGES.md` | `[CHANGE]` エントリ 1 件追記 |

## 完了条件

- `Error` 型の全文字列フィールドが `String` になり、動的な値を含むエラーメッセージを構築できること
- `Option` を返していた内部関数（`take_user_data` / `take_pending_decode` / `extract_h264_params` / `extract_h265_params` / `vec_u8_from_raw_parts_safe`）が `Result<_, Error>` になり、エラー情報がハンドラ経由でユーザーに伝搬されること
- 「対象外」に挙げた 4 箇所以外の `tracing::error!()` がすべて削除されていること
- 関連するテストの修正が完了し、 `cargo test` がパスすること
- `CHANGES.md` の `## develop` セクションに `[CHANGE]` エントリが追記されていること

## 解決方法

`Error` 型の全 `&'static str` フィールドを `String` に変更し、動的な値（実測値・異常値等）をエラーメッセージに含められるようにした。

- `src/error.rs`: 全 7 フィールドを `String` に変更、`Error::check()` の引数を `impl Into<String>` に変更、`LimitExceeded` の rustdoc を「防御的な上限超過や異常値（null ポインタ等）」に更新
- `src/encoder.rs`: `take_user_data` / `extract_h264_params` / `extract_h265_params` / `vec_u8_from_raw_parts_safe` を `Result<_, Error>` に変更、`process_encoded_output` の処理順序を `callback_from_ref_con` → `take_user_data` に入れ替え、全構築箇所に `.into()` を追加、`tracing::error!()` 12 箇所を削除
- `src/decoder.rs`: `take_pending_decode` を `Result<_, Error>` に変更、`output_callback` の処理順序を入れ替え、全構築箇所に `.into()` を追加、`tracing::error!()` 1 箇所を削除
- `src/types.rs`: `Error` 構築箇所に `.into()` を追加
- `tests/test_encoder.rs`: リテラルパターンマッチ 21 箇所を match guard に変更
- `tests/test_decoder.rs`: リテラルパターンマッチ 2 箇所を match guard に変更
- `tests/test_error.rs`: `Error` 構築箇所に `.into()` を追加
- `CHANGES.md`: `[CHANGE]` エントリを追記

なお、`LimitExceeded` への null ポインタ等の異常値の割り当ては `copy_plane` と同じ既存の慣行に従う。ブランチ名は後方互換のない変更を含むため `feature/change-reduce-log-output` とした。`cargo test --workspace` は全 33 テストがパスする。
