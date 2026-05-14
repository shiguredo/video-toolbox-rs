# ログ出力を削減し、エラー情報をユーザーに伝搬する

Created: 2026-05-13
Model: deepseek-v4-pro

## 方針

ユーザーにエラーの情報を `Result<>` で伝えられる場合、 `log::error!()` は不要とする。

## 背景・根拠

現在 `encoder.rs`・`decoder.rs` 内の複数の関数が `Option` を返しつつ `log::error!()` でエラー情報を出力している。これらはログでしか状況を伝える手段がないために存在するが、 `Result<_, Error>` に変更すればエラー情報をコールバック経由でユーザーに伝搬できるため、ログ出力は不要になる。

また `Error` 型の全文字列フィールドが `&'static str` に制約されているため、動的な値（例: 実際の `block_len` の数値、NAL ヘッダ長の実値、プレーン名の動的生成等）をエラーメッセージに含められず、 `log::error!()` で補足情報を出力せざるを得ない箇所が多い。フィールド型を `String` に一括変更し、 `format!()` による動的なエラーメッセージ構築を可能にする。

現在 `log::error!()` が存在する箇所（全 17 箇所）:

- encoder.rs: 865, 876, 978, 1042, 1057, 1061, 1086, 1106, 1121, 1125, 1155, 1249, 1259
- decoder.rs: 408, 419, 507
- types.rs: 53

なお encoder.rs の Drop 実装には `log::error!()` は含まれていない。また issue 0042（エラー時のユーザーデータ付与検討）は本 issue と関連するが独立した検討事項であり、本 issue の範囲外とする。

本 issue の前提として、 issue 0039（trait ベースのハンドラ）および issue 0040（ハンドラの型パラメータ変更）が適用済みのコードベースを想定する。

## CHANGES.md 追記方針

本変更は Error 型の公開フィールド型変更を含む破壊的変更であるため、 `[CHANGE]` として以下の 1 エントリを `CHANGES.md` の `## develop` セクションに追記する:

- `[CHANGE]` `Error` 型の全 `&'static str` フィールドを `String` に変更し、エラーメッセージに動的な値を含められるようにする
  - `Option` を返していた内部関数を `Result<_, Error>` に変更し、エラー情報をコールバック経由でユーザーに伝搬する
  - 不要になった `log::error!()` を削除する
  - @melpon

## 対応内容

### 1. Error 型の全 `&'static str` フィールドを `String` に変更する

現状の `Error` 型（`src/error.rs:4-51`）では以下の全 7 フィールドが `&'static str` に制約されている:

- `VideoToolbox.function`
- `InsufficientFrameData.plane`
- `UnsupportedCodec.codec`
- `InvalidConfig.field`, `InvalidConfig.reason`
- `LimitExceeded.reason`
- `CfObjectCreationFailed.function`

フィールド型を一律 `&'static str` のままにしておくと、今後動的な値を含めたくなったときに都度破壊的変更が必要になる。すべて `String` に一括変更する。

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

これにより `format!()` で動的な値を含むエラーメッセージを構築できるようになり、 `log::error!()` で補足情報を出力する必要がなくなる。

#### Error::check() のシグネチャ変更

`Error::check()` （error.rs:54）は `function: &'static str` を受け取り `Error::VideoToolbox { function }` を生成している。 `VideoToolbox.function` を `String` に変更するのに合わせ、パラメータ型を `function: impl Into<String>` に変更する:

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

`Display` 実装（error.rs:62-108）は全フィールドを `write!(f, "...{field}...")` 形式で表示しており、 `&'static str` も `String` も `Display` を実装しているため、型変更後もコード修正は不要。

#### API 互換性

`Error` は `pub use error::Error` で公開されている。全 `&'static str` フィールドの `String` 変更は破壊的変更であり、 `[CHANGE]` として扱う。ユーザーが `Error::InvalidConfig { field: "fps", reason: "zero" }` のように構築・パターンマッチしているコードでは、各リテラルを `.to_string()` に変更するか、 match guard で比較する必要がある。

#### 構築箇所の列挙

`String` 化により `.to_string()` または `String::from(...)` が必要になる全箇所:

- `types.rs:9-31` `validate_video_dimensions_for_toolbox` — 4 箇所（`InvalidConfig { field, reason }`）
- `types.rs:93-97` `cf_dictionary` — 1 箇所（`CfObjectCreationFailed { function }`）
- `types.rs:109-113` `cf_number_i32` — 1 箇所（同上）
- `types.rs:126-130` `cf_number_i64` — 1 箇所（同上）
- `types.rs:142-146` `cf_number_f64` — 1 箇所（同上）
- `encoder.rs:451-477` `validate_config` — 4 箇所（`InvalidConfig { field, reason }`）
- `encoder.rs:494-569` `copy_plane` — 10 箇所（`LimitExceeded { reason }`）
- `encoder.rs:577-626` `validate_frame_data` — 6 箇所（`InsufficientFrameData { plane, expected, actual }`）
- `encoder.rs:630-633` `frame_byte_len_checked` — 1 箇所（`LimitExceeded { reason }`）
- `encoder.rs:955-1013` `process_encoded_output` — 5 箇所（`LimitExceeded { reason }`）
- `encoder.rs:751-752` `encode` / `encode_pixel_buffer` PTS overflow — 2 箇所（`LimitExceeded { reason }`）
- `decoder.rs:153,157` `wrap_unsupported_codec_error` — 2 箇所（`UnsupportedCodec { codec }`）
- `decoder.rs:467-468` `output_callback` — 1 箇所（`LimitExceeded { reason }`）

### 2. `Option` を返している関数を `Result<_, Error>` に変更する

現状 `Option` を返しつつ `log::error!()` でエラー内容を出力している関数を `Result` に変更し、エラー情報を呼び出し元に伝搬する。

#### 2.1 `vec_u8_from_raw_parts_safe` (encoder.rs:1243)

現在の戻り値: `Option<Vec<u8>>`

| エラーパス | 使用する Error バリアント |
|---|---|
| `len > MAX_PARAMETER_SET_COPY_BYTES` | `LimitExceeded { reason: format!("{context}: parameter set length {len} exceeds defensive maximum {max}", max = MAX_PARAMETER_SET_COPY_BYTES) }` |
| `ptr.is_null() && len > 0` | `LimitExceeded { reason: format!("{context}: null pointer with non-zero length") }` |

#### 2.2 `extract_h264_params` (encoder.rs:1037)

現在の戻り値: `Option<ParameterSets>`

| エラーパス | 使用する Error バリアント |
|---|---|
| description.is_null() | `LimitExceeded { reason: "CMVideoFormatDescription is null in extract_h264_params".into() }` |
| CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (1 回目) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetH264ParameterSetAtIndex".into() }` |
| nalu_header_length != 4 | `LimitExceeded { reason: format!("unexpected NAL unit header length: {nalu_header_length}") }` |
| CMVideoFormatDescriptionGetH264ParameterSetAtIndex 失敗 (2 回目) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetH264ParameterSetAtIndex".into() }` |
| vec_u8_from_raw_parts_safe 失敗 | 伝搬（`?` 演算子） |

関数ポインタ型の変更: `process_encoded_output` の引数 `extract_params` の型を `unsafe fn(sys::CMVideoFormatDescriptionRef) -> Option<ParameterSets>` から `unsafe fn(sys::CMVideoFormatDescriptionRef) -> Result<ParameterSets, Error>` に変更する。

#### 2.3 `extract_h265_params` (encoder.rs:1101)

現在の戻り値: `Option<ParameterSets>`

| エラーパス | 使用する Error バリアント |
|---|---|
| description.is_null() | `LimitExceeded { reason: "CMVideoFormatDescription is null in extract_h265_params".into() }` |
| CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (1 回目) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex".into() }` |
| nalu_header_length != 4 | `LimitExceeded { reason: format!("unexpected NAL unit header length: {nalu_header_length}") }` |
| CMVideoFormatDescriptionGetHEVCParameterSetAtIndex 失敗 (2 回目以降) | `VideoToolbox { status, function: "CMVideoFormatDescriptionGetHEVCParameterSetAtIndex".into() }` |
| vec_u8_from_raw_parts_safe 失敗 | 伝搬（`?` 演算子） |

#### 2.4 `take_user_data` (encoder.rs:860)

現在の戻り値: `Option<H::UserData>`

| エラーパス | 使用する Error バリアント |
|---|---|
| source_frame_ref_con.is_null() | `LimitExceeded { reason: format!("{callback_name}: source_frame_ref_con is null") }` |

#### 2.5 `take_pending_decode` (decoder.rs:403)

現在の戻り値: `Option<Box<PendingDecode<H::UserData>>>`

| エラーパス | 使用する Error バリアント |
|---|---|
| source_frame_ref_con.is_null() | `LimitExceeded { reason: format!("{callback_name}: source_frame_ref_con is null") }` |

### 3. FFI コールバックの処理順序を変更する

現在の FFI コールバックは `take_user_data`/`take_pending_decode` → `callback_from_ref_con` の順で呼んでいるため、 `take_user_data`/`take_pending_decode` が失敗した時点でハンドラが未取得でありエラーを伝搬できない。

処理順序を `callback_from_ref_con` → `take_user_data`/`take_pending_decode` に変更し、 `take_user_data`/`take_pending_decode` の失敗をハンドラ経由でユーザーに伝搬できるようにする。

対象箇所:
- encoder.rs: `process_encoded_output`（929 行付近）
- decoder.rs: `output_callback`（433 行付近）

#### 安全性の検証

2 つのポインタ（`source_frame_ref_con` と `output_callback_ref_con`）は独立しており、 `Box::from_raw` と `&mut *ptr` のライフタイム競合は発生しない。また Video Toolbox はセッション単位でコールバックを直列化するため、処理順序入れ替え中に別コールバックが割り込むことはない。

`callback_from_ref_con` が null で早期 return する場合、 `source_frame_ref_con` の `Box` が `Box::from_raw` で回収されずリークする。このパスは実運用で到達しない異常系だが、リーク防止のため早期 return 前に `source_frame_ref_con` が非 null であれば `Box::from_raw` で消費する。

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

### 4. 上記変更に伴い不要になった `log::error!()` を削除する

以下の 13 箇所の `log::error!()` を削除する:

| ファイル | 行 | 理由 |
|---|---|---|
| encoder.rs | 865 | `take_user_data` → `Result` 化でハンドラ経由伝搬 |
| encoder.rs | 978 | `LimitExceeded.reason` に動的値を含めて伝搬 |
| encoder.rs | 1042 | `extract_h264_params` → `Result` 化で伝搬 |
| encoder.rs | 1057 | 同上 |
| encoder.rs | 1061 | 同上 |
| encoder.rs | 1086 | 同上 |
| encoder.rs | 1106 | `extract_h265_params` → `Result` 化で伝搬 |
| encoder.rs | 1121 | 同上 |
| encoder.rs | 1125 | 同上 |
| encoder.rs | 1155 | 同上 |
| encoder.rs | 1249 | `vec_u8_from_raw_parts_safe` → `Result` 化で伝搬 |
| encoder.rs | 1259 | 同上 |
| decoder.rs | 408 | `take_pending_decode` → `Result` 化で伝搬（順序入れ替え後） |

## 対象外

以下はエラーの伝搬先が存在しないため、ログ出力を残す:

- `callback_from_ref_con` の null チェック（encoder.rs:876 / decoder.rs:419）: ハンドラポインタ自体が null であり伝搬先がない
- Drop 実装内のエラー:
  - decoder.rs:507（`Decoder::drop` 内の `self.finish()` 失敗）
  - types.rs:53（`CvPixelBufferUnlockGuard::drop` 内のアンロック失敗）

エンコーダー側の Drop 実装（encoder.rs:1169-1176）は `VTCompressionSessionInvalidate` + `CFRelease` のみで `log::error!()` を含んでおらず、対応不要。

## テスト戦略

### 単体テスト

`Error` の `String` フィールド化に伴い、以下のテスト修正が必要:

- **`tests/test_encoder.rs`**: `matches!()` マクロでのパターンマッチを修正する
  - `Error::InvalidConfig { field: "width", .. }` 等、 `reason` を `..` で無視している 9 箇所は変更不要
  - `Error::InvalidConfig { field: "fps_numerator", reason: "must not be zero" }` （1 箇所）: `reason` が `String` になるため `&str` リテラルと直接マッチできなくなる。 match guard を用いて `reason if reason == "must not be zero"` に変更する
- **`tests/test_decoder.rs`**: `Error::InvalidConfig { field: "width", .. }` 等 `reason` を無視している 2 箇所は変更不要
- **`tests/test_error.rs`**: リテラルによる `Error` 構築箇所に `.into()` を追加する
- **手戻り防止**: テスト修正後に `cargo test` で全テストがパスすることを確認する

### PBT / Fuzzing

本変更ではロジックの追加や入力検証の変更を伴わないため、新規 PBT および fuzzing は不要。

## 変更対象ファイル一覧

| ファイル | 変更内容 |
|---|---|
| `src/error.rs` | 全 `&'static str` フィールドを `String` に変更、 `Error::check()` のパラメータ型を `impl Into<String>` に変更 |
| `src/encoder.rs` | `take_user_data`/`extract_h264_params`/`extract_h265_params`/`vec_u8_from_raw_parts_safe` → `Result` 化、関数ポインタ型変更、`process_encoded_output` の処理順序入れ替え、全構築箇所の `.into()` 追加、 `log::error!()` 13 箇所削除 |
| `src/decoder.rs` | `take_pending_decode` → `Result` 化、 `output_callback` の処理順序入れ替え、全構築箇所の `.into()` 追加、 `log::error!()` 1 箇所削除 |
| `src/types.rs` | `Error` 構築箇所の `.into()` 追加（`validate_video_dimensions_for_toolbox`、 `cf_dictionary`、 `cf_number_*`） |
| `tests/test_encoder.rs` | `matches!()` の match guard 修正（1 箇所）、 `Error` 構築箇所の `.into()` 追加 |
| `tests/test_decoder.rs` | 変更不要（`reason` を `..` で無視しているため影響なし）の確認 |
| `tests/test_error.rs` | `Error` 構築箇所の `.into()` 追加 |
| `CHANGES.md` | `[CHANGE]` エントリ 1 件追記 |
