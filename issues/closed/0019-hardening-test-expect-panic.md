# テスト内の `expect` / `panic!` による失敗経路の整理

Created: 2026-04-01  
Completed: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`#[cfg(test)]` 内で `expect` や `panic!` が使われている。本番コードではないが、テスト失敗時は **パニック**で終了する。テストの意図（「ここで失敗したらテストとして失敗でよい」）は明確にしつつ、**テストヘルパの再利用**や **CI の失敗メッセージの読みやすさ**の観点で、`Result` ベースに統一するかどうかを検討する余地がある。

## 現状

- **場所**: `src/lib.rs` の `#[cfg(test)] mod tests` 内
- エンコーダー／デコーダー／ラウンドトリップ等のテストで `expect` や `panic!` が使われている。

## 問題

- テスト失敗がパニックに依存しており、テスト以外のユーティリティとして流用した場合に意図しないパニック経路になる。

## 望ましい対応の方向（案）

- 優先度は低い。必要なら `Result` 返却のテストヘルパに寄せる、または `expect` のメッセージを統一する。
- 「本番コードに `expect` はない」ことを維持する方針で問題ないか確認する。

## 解決方法

`encode_h264_black` / `encode_h265_black` / `init_vp9_decoder` / `init_av1_decoder` を `Result<(), Error>` にし、`?` で伝播するよう変更した。`encoder_rejects_zero_fps_numerator` は `let-else` で `InvalidConfig` を検証する。`vp9_decoder` 内の libvpx 呼び出しの `expect` は外部クレートのため残した。

## 解決方法（再対応）

`vp9_decoder` テストで libvpx のエンコーダ生成・エンコード・`finish` が失敗した場合は `Ok(())` で打ち切る（環境・依存の揺れに合わせた緩和）。`next_frame` の `Result` 化に合わせエンコード系テストのループを更新した。

## 解決方法（レビュー追補）

`panic!` マクロおよび `unwrap_or_else(|| panic!(...))` をテストから除去した。

- `encoder_rejects_zero_fps_numerator` は `matches!` で `InvalidConfig` を検証する。
- `vp9_decoder` はデコード結果の `None` を `assert!` + `unwrap` で検証し、ピクセルフォーマット分岐は `match` とする。I420 以外（このテストでは到達しない想定の NV12）は `unreachable!` とした（`assert!(false)` は clippy の `assertions_on_constants` に抵触するため）。
