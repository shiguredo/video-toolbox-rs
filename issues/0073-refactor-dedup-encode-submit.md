# `encode` / `encode_pixel_buffer` の重複を解消する

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-dedup-encode-submit
- Polished: 2026-08-01

## 目的

`src/encoder.rs` の `encode` と `encode_pixel_buffer` で、`frame_properties` 構築・`VTCompressionSessionEncodeFrame` 呼び出し・エラー時の Box 回収・`next_input_pts` 加算が約 40 行にわたり同一。バグ修正時に片方だけ漏れるリスクがある（issue 0061 の PTS 検査順序修正がまさに両方に必要）。

## 現状

両メソッドで以下のブロックが完全に重複（`encode` 側にのみ status エラー時の Box 回収理由のコメントがある 1 行を除く）:

- `frame_properties` の `cf_dictionary` 構築とガード
- `source_frame_ref_con` の `Box::into_raw`
- `VTCompressionSessionEncodeFrame` 呼び出しとエラー時の Box 回収
- `next_input_pts` の `checked_add`

## 設計方針

「CVPixelBuffer を受け取ってエンコードに投入する」共通 private メソッド（例: `submit_pixel_buffer`）を抽出し、両メソッドはバッファの取得方法と検証（`encode`: `FrameData` バリアントのフォーマット・フレーム長検証、`encode_pixel_buffer`: `CVPixelBufferGetPixelFormatType` によるフォーマット検証）だけ担当する形にする。共通メソッドには `encode` 側の status エラー時の Box 回収理由のコメントを移して維持する。

共通メソッドの内部では、issue 0061 が定めた PTS 検査順序（`checked_add` を `Box::into_raw` より前、すなわち `user_data` の Box 化より前に実行）を維持する。共通メソッドは `CfPtrMut<sys::__CVBuffer>` を値で受け取る。

### 検証方法

`sys::VTCompressionSessionEncodeFrame(` の呼び出しが `src/` 内で 1 箇所に集約されていることを grep で確認する（`VTCompressionSessionEncodeFrame` という文字列はコメントや `Error::check` の文字列リテラルにも現れるため、呼び出し形式で grep する）。挙動変更がないことを既存テストの回帰で確認する。なお issue 0053 実施後は `encode` 系が `pixel_buffer.rs` へ移動するため、grep は `src/` 全体を対象にする。

## 完了条件

- `VTCompressionSessionEncodeFrame` 呼び出し周りのロジックが 1 箇所に集約されていること
- 公開 API の変更がないこと。挙動変更がないこと（リファクタリングのみ）
- `CHANGES.md` の `## develop` に `[UPDATE]`（`### misc`）としてエントリを追記する
- `cargo test --workspace -- --test-threads=1` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` が通る

## 関連 issue

- issue 0061: PTS 検査順序の修正（`checked_add` を送信前に移動）。本 issue は 0061 の修正が適用済みであることを前提とし、0061 を先に実施してから本 issue を実施する
- issue 0053: `encode` 系の `pixel_buffer.rs` への移動が同一領域に触れる。本 issue の共通メソッドも移動対象になる
- issue 0055: `frame_properties` の構築と同じ行域を対象とする。どちらを先に実施しても成立する
- issue 0071: `encode_pixel_buffer` のテスト追加が同一領域に触れる
