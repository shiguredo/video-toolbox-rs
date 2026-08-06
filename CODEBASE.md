# CODEBASE

本リポジトリ固有の規約・例外を集約する。共通規約は `AGENTS.md` と `shiguredo-rust` スキルを参照すること。

## トレイト定義の許可

- `shiguredo-rust` 規約は原則としてトレイト定義を禁止しているが、本クレートでは以下の 2 つの公開トレイトを**許可済み**として維持する
  - `encoder::EncodeHandler`
  - `decoder::DecodeHandler`
- 許可の根拠
  - shiguredo のビデオコーデックバックエンド 4 クレート
    （`nvcodec-rs` / `vpl-rs` / `amf-rs` / 本クレート）で
    公開 API のコールバックモデルを揃えている。関連型名 (`UserData`, `Error`)、
    メソッド名 (`on_encoded` / `on_decoded`)、クロージャラッパー
    (`FnEncodeHandler` / `FnDecodeHandler`)、`Encoder<H: EncodeHandler>` /
    `Decoder<H: DecodeHandler>` の型シグネチャまでシグネチャレベルで一致しており、
    利用側（Hisui 等）はバックエンドを切り替えても同じ形でコードを書ける
  - このエコシステム全体の統一性は、単一クレートの設計優劣より優先する
  - Video Toolbox はエンコード / デコード完了を FFI コールバック経由で通知する非同期 API であり、
    呼び出し側が任意の状態（ユーザーデータ・エラー型）を保持しつつコールバックを受ける必要がある。
    関連型 (`type UserData`, `type Error`) をもつトレイトはこの用途に自然な表現である
- 上記 4 クレートのモデルを変更する場合は、必ず全クレートで同時に変更すること。
  本クレート単独で変更してはならない
- 新たにトレイトを追加する場合は、必ず本ドキュメントに理由と共に追記すること

## re-export の許可

- `shiguredo-rust` 規約は原則として re-export を禁止しているが、本クレートでは
  `encoder` モジュールのサブモジュール分割に伴い、公開 API のパス維持のため
  `src/encoder.rs` での `pub use` による再公開を**許可済み**とする
- 許可の根拠
  - `src/encoder.rs` をディレクトリモジュール (`src/encoder/`) に分割した際、
    `EncoderConfig` / `ReconfigureParams` / `FrameData` / `EncodeHandler` 等の公開型が
    サブモジュール (`config` / `frame` / `handler`) に移動した
  - `src/lib.rs` の `pub use encoder::{...}` と外部呼び出し側 (`use shiguredo_video_toolbox::Encoder`)
    のパスを変えずに分割を実現するため、親モジュールでの re-export が必要
  - モジュール構造の内部変更（分割）で公開 API のパスを維持するための例外的な許可であり、
    新たな re-export を追加する場合は、必ず本ドキュメントに理由と共に追記すること

## catch_unwind の許可

- `shiguredo-rust` 規約は「`std::panic::catch_unwind` を使わないこと」を定めているが、
  本クレートでは `extern "C"` コールバック内でのユーザーハンドラの panic 捕捉に限り
  **許可済み**とする
- 許可の根拠
  - エンコーダー / デコーダーの FFI コールバック (`output_callback_h264` / `output_callback_h265` /
    `output_callback`) は `extern "C"` 関数として定義されており、ユーザー実装の
    `on_encoded` / `on_decoded` が panic すると unwind が ABI 境界を越えてプロセスが abort する
  - ユーザーハンドラの panic は実装バグの表明ではなく利用者コードの実行結果であり、
    `extern "C"` 境界を越えさせないための捕捉であって握りつぶしではない
  - 捕捉した panic はエラーログ（コールバック名 + panic メッセージ）で可視化し、
    セッションは継続する
  - ホストアプリが abort するカスタム panic hook をインストールしている場合や
    `panic=abort` ビルドでは `catch_unwind` では捕捉されず abort するため、
    本保証はこれらの環境では成立しない
- 新たに `catch_unwind` を使う場合は、必ず本ドキュメントに理由と共に追記すること

## テストの前提

- 本クレートは Video Toolbox の実 FFI を叩くため、テスト実行には macOS が必須
- CI はセルフホスト（`macOS` / `ARM64`）で動作する。詳細は `README.md` を参照
