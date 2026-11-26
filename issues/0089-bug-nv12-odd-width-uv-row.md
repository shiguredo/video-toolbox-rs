# NV12 の奇数幅で UV プレーンのクロマ行が 1 バイトずつずれる

- Created: 2026-11-25
- Completed:
- Branch: feature/fix-nv12-odd-width-uv-row
- Polished:

## 目的

`Encoder::encode` に `FrameData::Nv12` を渡すとき、`EncoderConfig.width` が奇数だと
UV プレーンの行が 1 バイトずつずれてクロマが壊れる。エラーにならず U と V が
入れ替わった色でエンコードされるため、利用者が気付かないまま映像品質を損なう。
奇数幅の NV12 入力を正しい NV12 の行レイアウトで扱えるようにする。

## 現状

### 原因

`src/encoder/pixel_buffer.rs` の `validate_frame_data` は、NV12 の UV プレーン長を
`width * ceil(height / 2)` としている。また `Encoder::encode` は `copy_plane` に
UV プレーンのコピー元の行幅と行間隔として `width` を渡している。

NV12 の UV プレーンは U と V が 1 サンプルずつ交互に並ぶため、1 行のバイト数は
`2 * ceil(width / 2)` である。偶数幅では `width` と等しいが、奇数幅では
`width + 1` になる。このため奇数幅では、正しい NV12 のバッファを渡すと
2 行目以降の読み出し開始位置が 1 バイトずつ手前にずれ、クロマ行の奇数行で
U と V が入れ替わる。各行の最後の V（位置 `width`）もコピーされない。

`Encoder::encode_pixel_buffer` は CVPixelBuffer 自身の行レイアウトを使うため影響を受けない。
I420 はクロマ幅に `ceil(width / 2)` を正しく使っているため影響を受けない。

### 再現手順

確認環境: macOS 26.6.2 / Apple M4 Pro、H.264、`real_time = true`、
`average_bitrate = Some(2_000_000)`、30 fps、フレーム再順序付けなし、時間的圧縮なし。

1. `width = 999`、`height = 500`、`pixel_format = PixelFormat::Nv12` で `Encoder` を生成する。
2. UV プレーンを正しい NV12 の行幅 `2 * ceil(999 / 2) = 1000` バイトで用意し、
   U = 64、V = 192 の一様なクロマにする。Y は 16 で埋める。
3. `Encoder::encode` に `FrameData::Nv12` として渡し、キーフレームを 1 枚エンコードして
   `Encoder::finish` を呼ぶ。
4. 出力を `Decoder`（`PixelFormat::Nv12`）でデコードし、UV プレーンのクロマ行ごとの
   平均 U / V を求める。

結果:

- クロマ行の偶数行は U ≈ 64 / V ≈ 192 のまま。
- クロマ行の奇数行は U ≈ 191 / V ≈ 66 となり、U と V が入れ替わる。
- デコード後の 250 クロマ行のうち 126 行で、`Encoder::encode_pixel_buffer` の結果と一致しない。
- 同じ画像を `Encoder::encode_pixel_buffer` で送ると、すべての行が U = 64 / V = 192 になる。
- `width = 1000` の対照では両経路が一致する。
- エンコード後のバイト数は、スライス入力経路が 594 バイト、直接入力経路が 204 バイトだった。

Video Toolbox は 999x500 の入力を 998x500 としてエンコードする。そのため、
行の最後の V の欠落自体はデコード結果に現れないが、行ずれは内側のクロマに影響する。

### 影響

- 影響する: `EncoderConfig.width` が奇数で、`PixelFormat::Nv12` の `Encoder` に
  `FrameData::Nv12` を渡す場合。
- 影響しない: `Encoder::encode_pixel_buffer`、I420、偶数幅。
- 奇数幅は `Encoder::new` の設定検証で拒否されていない。`CVPixelBufferCreate` も
  奇数幅を受け付けるため、この経路は到達可能である。
- エラーにならないため、利用者はクロマの破損に気付きにくい。

## 設計方針

NV12 の UV 行のバイト数を `uv_row_bytes = 2 * width.div_ceil(2)` として、
検証とコピーの両方で使う。偶数幅では従来と同じ値になり、奇数幅では 1 バイト増える。

- `validate_frame_data` の NV12 分岐: `uv_row_bytes * ceil(height / 2)` を要求する。
- `Encoder::encode` の UV コピー: `copy_plane` のコピー元の行幅と行間隔に
  `uv_row_bytes` を渡す。
- `FrameData::Nv12` の rustdoc に、UV プレーンは 1 行 `2 * ceil(width / 2)` バイトの
  詰め物なしレイアウトであることを明記する。

奇数幅の NV12 を拒否する案もあるが、I420 は奇数幅を扱えており、Video Toolbox も
奇数幅の CVPixelBuffer を受け付けるため、入力レイアウトを正しく扱う方を採る。

現在の検証式に合わせて `width * ceil(height / 2)` バイトの UV を渡していた利用者は、
修正後に `Error::InsufficientFrameData` を受け取る。そのバッファは正しい NV12 の
行レイアウトではないため、エラーにするのが正しい。

## 完了条件

- `width` が奇数の NV12 フレームで、`Encoder::encode` の出力が
  `Encoder::encode_pixel_buffer` と同じクロマになる。
- 偶数幅の NV12 と I420 の既存の挙動が変わらない。
- `validate_frame_data` が奇数幅の NV12 で
  `2 * ceil(width / 2) * ceil(height / 2)` を要求することを検証するテストがある。
- 999x500 の NV12 をエンコード・デコードしてクロマを比較する回帰テストがあり、
  修正前は失敗し、修正後は成功する。
- `cargo test --workspace -- --test-threads=1`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo fmt --all -- --check` が成功する。
- `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する。
