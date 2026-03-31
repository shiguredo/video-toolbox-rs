# fps_denominator が 0 の場合にゼロ除算パニックが発生する

Created: 2026-03-31
Completed: 2026-03-31
Model: Opus 4.6

## 概要

`EncoderConfig::fps_denominator` が `0` の場合、`add_common_properties()` 内の `config.fps_numerator.div_ceil(config.fps_denominator)` でゼロ除算パニックが発生する。

公開 API (`Encoder::new()`, `Encoder::reconfigure()`) の入り口で `Result` としてエラーを返すべきところが、パニックで落ちる。

## 該当箇所

- `src/lib.rs:407` — `div_ceil(config.fps_denominator)` の呼び出し
- `src/lib.rs:271` — `Encoder::new()` から到達
- `src/lib.rs:292` — `Encoder::reconfigure()` から到達

## 再現手順

```rust
let config = EncoderConfig {
    fps_numerator: 30,
    fps_denominator: 0,
    // ...
};
let encoder = Encoder::new(config); // パニック
```

## 修正方針

`Encoder::new()` と `Encoder::reconfigure()` の先頭で `fps_denominator == 0` を検証し、`Error` として返す。

## 解決方法

`Encoder::validate_config()` メソッドを追加し、`Encoder::new()` と `Encoder::reconfigure()` の先頭で呼び出すようにした。`fps_denominator == 0` の場合は `Error::InvalidConfig` を返す。
