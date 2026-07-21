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

## テストの前提

- 本クレートは Video Toolbox の実 FFI を叩くため、テスト実行には macOS が必須
- CI はセルフホスト（`macOS` / `ARM64`）で動作する。詳細は `README.md` を参照
