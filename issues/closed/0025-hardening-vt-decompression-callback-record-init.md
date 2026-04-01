# `VTDecompressionOutputCallbackRecord` の初期化方針をコード内で明示する

Created: 2026-04-01  
Model: Composer 2 Fast

## なぜこの対応が必要か

`Decoder::create_decompression_session` では `MaybeUninit::<VTDecompressionOutputCallbackRecord>::zeroed().assume_init()` で構造体値を作ったうえで、`decompressionOutputCallback` だけを代入している。

`MaybeUninit::zeroed()` に続く `assume_init()` は **ゼロビット列を有効な `VTDecompressionOutputCallbackRecord` として読む**という意味であり、「**新フィールドが未定義のまま渡る**」という説明は **不正確**である（ゼロ初期化された値が渡る）。問題にするなら **ゼロ初期化に依存する方針そのものを、保守者が誤解なく追えるようにコメントで明示する**ことで足りる。

`#[repr(C, packed(4))]` の bindgen 定義と、`&callback` を **`VTDecompressionSessionCreate` に渡す**ことは別論点である。**構造体全体への参照を FFI に渡す**ことと、**packed な各フィールドへの参照の取り扱い**を同一の「packed UB」として混ぜるのは不適切なので、本 issue では **初期化方針の明示**に範囲を絞る。

## 現状

- **場所**: `src/lib.rs` の `Decoder::create_decompression_session`（`MaybeUninit::zeroed().assume_init()` の直後に `decompressionOutputCallback` のみ代入し、`&callback` を `VTDecompressionSessionCreate` に渡している）
- bindgen 上、`VTDecompressionOutputCallbackRecord` は **`decompressionOutputCallback` と `decompressionOutputRefCon` の 2 メンバ**（現行ヘッダ前提）。ゼロ初期化により **両フィールドはゼロ**であり、関数ポインタは NULL、`decompressionOutputRefCon` は NULL のまま。その後 **`decompressionOutputCallback` だけ `Some(Self::output_callback)` で上書き**している。`output_callback` 本体は第 1 引数 `_decompression_output_ref_con` を **未使用**（`source_frame_ref_con` のみ使用）。

## 問題（リスク）

- **なぜゼロ初期化してから一部フィールドだけ代入するのか**が、コード上だけでは追いにくい（将来のメンテで別初期化に変えたときの意図が読み取れない）。

## 望ましい対応の方向（案）

- `create_decompression_session` 内に **日本語コメント**で、（1）現行 SDK では 2 フィールドのみであること、（2）ゼロは refcon を NULL にする意図であること、（3）コールバックのみ上書きする理由、を簡潔に書く。
- 必要なら **メンバごとに `MaybeUninit::uninit()` + 個別代入**に変え、**ゼロに依存しない**書き方にする（どちらか一方でよい）。

## 解決の完了条件

- 上記の **意図がコメントまたはコードのどちらかで追試可能**であること。
- **誤った前提**（「未定義フィールドが残る」等）を含む説明を issue・コメントから **排除**すること。

## レビュー指摘への反映（2026-04-01）

- 旧本文の「新フィールドが未定義のまま」は **事実と異なる**ため削除した。
- `packed` と **`&callback` の FFI 渡し**を **同一の危険度**として扱う記述は **削除**し、本 issue のスコープを **初期化方針の明示**に限定した。

## レビュー確認（指摘とソース照合）

| 指摘 | 確認結果 |
|------|----------|
| `zeroed().assume_init()` が「未定義のまま渡す」わけではない | **妥当**。`zeroed()` は全ビット 0 の値を作り、`assume_init()` はその**初期化済み**値として読む。SDK でフィールドが増えた**将来**の話は、別途「ゼロがその型で常に妥当か」をヘッダで確認する論点であり、**「未初期化が残る」という意味ではない**。 |
| `packed` と `&callback` の FFI 渡しを packed フィールド参照の UB と同列にしない | **妥当**。本 issue は **初期化方針のコメント化**に限定する。 |
| `src/lib.rs` 1473–1491 行付近の実装 | 上記 **現状**の記述と一致。 |

## 解決方法

Completed: 2026-04-01

- `create_decompression_session` 内の `MaybeUninit::<VTDecompressionOutputCallbackRecord>::zeroed().assume_init()` の直前に、現行 SDK の 2 フィールド構成とゼロ初期化の意図を日本語コメントで明示した。
