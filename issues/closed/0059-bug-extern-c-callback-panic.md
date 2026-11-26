# `extern "C"` コールバック内の panic でプロセスが abort する

- Created: 2026-07-30
- Completed: 2026-08-06
- Branch: feature/fix-extern-c-callback-panic
- Polished: 2026-08-01

## 目的

エンコーダー・デコーダーの FFI コールバック（`output_callback_h264` / `output_callback_h265` / `output_callback`）は `extern "C"` 関数として定義されている。ユーザー実装の `on_encoded` / `on_decoded` が panic すると、unwind が `extern "C"` の ABI 境界を越えようとし、Rust 1.81 以降の現行ツールチェーンではプロセスが **abort** する（それ以前は未定義動作だった）。ユーザーコードの panic がアプリケーション全体を巻き込むため、捕捉して保護する。

## 現状

`src/encoder.rs` の `output_callback_h264` / `output_callback_h265` と `src/decoder.rs` の `output_callback` は `unsafe extern "C" fn` で定義されている。`invoke_callback` が FFI 境界内からユーザーの `on_encoded` / `on_decoded` を直接呼ぶため、panic の保護がない。`std::panic::catch_unwind` はソースに一切使われていない。

再現手順: `on_encoded` / `on_decoded` 内で panic するハンドラを登録して encode / decode を実行すると、プロセスが abort する。

## 設計方針

ユーザーハンドラの panic を `std::panic::catch_unwind` で捕捉してエラーログを出し、エンコード・デコードセッションは継続する。捕捉された panic はプロセスを abort させない。なお、ホストアプリが abort するカスタム panic hook をインストールしている場合や `panic=abort` ビルドでは `catch_unwind` は無効になり abort するため、本保証はこれらの環境では成立しない。

`extern "C-unwind"` への変更は採用しない。Rust 1.71 で安定化しているが、panic を Video Toolbox 内部へ伝播させるだけで、Video Toolbox は Rust の panic（unwind）を処理するコードを持たない。unwind 非対応でビルドされていれば未定義動作、対応していても未捕捉のまま abort するため、プロセスを守る観点では現状と同等かそれ以下であり、解決にならない。

### 規約の例外

shiguredo-rust 規約の「`std::panic::catch_unwind` を使わないこと」との衝突を、`CODEBASE.md` に例外を記載して解決する。根拠: ユーザーハンドラの panic は実装バグの表明ではなく利用者コードの実行結果であり、`extern "C"` 境界を越えさせないための捕捉であって握りつぶしではない。捕捉した panic はエラーログ（panic メッセージ + コールバック名）で可視化する。

### 捕捉後の挙動

`Box<H>` のハンドラはコールバック間で共有され、panic 後も後続フレームで再度呼ばれる。panic を捕捉したフレームはスキップして継続し、後続フレームで再 panic した場合は再度捕捉してエラーログを出す。

### 保護範囲

保護対象は `on_encoded` / `on_decoded` 内の panic のみ。ユーザーハンドラがフレームを即時 drop する典型的な使い方では `H::UserData` の `Drop` は `catch_unwind` 内で実行されるため、その `Drop` 実装内の panic は二重 panic で abort し得る（エラーパス・成功パスを問わない）。エラーパスで drop される `H::Error` の `Drop` 実装内の panic も同様。これらは本 issue の保証の範囲外とする。

### 変更対象

- `src/encoder.rs`: `invoke_callback` 内の `handler.on_encoded` 呼び出しを `catch_unwind` で包む（`&mut H` は `UnwindSafe` でないため `AssertUnwindSafe` が必要）。panic 捕捉時のエラーログにコールバック名を含めるため、`invoke_callback` へ `callback_name` 引数を追加し、呼び出し箇所（9 箇所）を更新する
- `src/decoder.rs`: 同様（`handler.on_decoded` を包み、`callback_name` 引数を追加して呼び出し箇所（5 箇所）を更新する）
- `src/types.rs`: `catch_user_panic`（encoder / decoder 共通の panic 捕捉関数）と `panic_payload_message`（panic メッセージ抽出）を追加
- `Cargo.toml`: テストのログ検証用に `tracing-subscriber` を dev-dependencies へ追加
- `CODEBASE.md`: `catch_unwind` 例外の根拠付き記載
- `CHANGES.md`: `[FIX]` エントリ

## 完了条件

- ユーザーハンドラが panic してもプロセスが abort しないこと（panic するハンドラを登録して encode / decode するテストで確認）
- panic 捕捉後もセッションが継続し、後続フレームのエンコード・デコード結果がハンドラに届くこと
- panic 捕捉時にエラーログが出力されること（検証には `tracing-subscriber` を dev-dependencies へ追加する必要がある）
- `CODEBASE.md` に `catch_unwind` 例外が根拠付きで記載されていること
- `CHANGES.md` の `## develop` に `[FIX]` としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0041: 同一のコールバック経路（`process_encoded_output` / `invoke_callback`）を変更対象とするため、実装順序と差分衝突に注意する
- issue 0053: `encoder.rs` のモジュール分割で `output_callback_h264` / `output_callback_h265` / `process_encoded_output` / `invoke_callback` が別ファイルへ移動するため、実装順序と差分衝突に注意する

## 解決方法

- `src/types.rs` に `catch_user_panic`（encoder / decoder 共通の panic 捕捉関数）を追加した
  - `std::panic::catch_unwind` + `AssertUnwindSafe` でユーザーハンドラの呼び出しを包み、捕捉した panic は
    `panic_payload_message` でメッセージを取り出して `tracing::error!` で「コールバック名 + panic メッセージ」のエラーログを出力する
  - 捕捉後のセッションは継続し、後続フレームのコールバックでハンドラは再度呼ばれる
  - `&Box<dyn Any + Send>` を `&(dyn Any + Send)` に渡すと deref されず Box 構造体自体が unsize されて
    downcast が失敗するため、`&*payload` で明示的に deref している
- `src/encoder/callback.rs` / `src/decoder.rs` の `invoke_callback` に `callback_name` 引数を追加し、
  `handler.on_encoded` / `handler.on_decoded` の呼び出しを `catch_user_panic` 経由に置き換えた
  （encoder 9 箇所 / decoder 5 箇所の呼び出し箇所を更新）
- 公開 API の `EncodeHandler` / `DecodeHandler` の doc に「panic しても abort せずエラーログが出て
  セッションが継続する」旨の保証を追記した（`panic=abort` ビルド等では成立しない但し書き付き）
- テスト
  - `tests/test_encoder.rs` に `handler_panic_is_caught_and_encode_continues` を追加した
    （H.264 エンコーダーで 1 回目のコールバックを panic させ、2 回目のフレームが届くことと
     `output_callback_h264: user handler panicked` のエラーログを検証）
  - `tests/test_decoder.rs` に `handler_panic_is_caught_and_decode_continues` を追加した
    （H.264 デコーダーで同様の検証）
  - `src/types.rs` に `panic_payload_message` の 3 分岐（`&str` / `String` / unknown）のテストを追加した
  - ログ収集は `tests/helpers.rs` の `LogCollector` / `init_global_log_collector`（グローバル subscriber）
    で行う（Video Toolbox のコールバックは別スレッドで実行されるため）
- `tracing-subscriber` を dev-dependencies に追加し、`CODEBASE.md` に「catch_unwind の許可」を
  根拠付きで追記した
- 完了条件の確認結果
  - panic するハンドラを登録した encoder / decoder のテストで、abort せずに panic が捕捉されることを確認
  - panic 捕捉後もセッションが継続し、後続フレームの結果がハンドラに届くことを確認
  - `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace -- -D warnings` /
    `cargo fmt --all -- --check` がすべて通ることを確認
