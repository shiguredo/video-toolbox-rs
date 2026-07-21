# PTS 再スケールの `new_timescale < old_timescale` 切り上げ効果を検証するテストを追加する

- Priority: Medium
- Created: 2026-05-14
- Updated: 2026-07-21
- Completed:
- Model: Opus 4.7
- Branch: feature/add-pts-rescale-downscale-boundary-test

## 目的

`Encoder::reconfigure` は `expected_frame_rate` 更新時に `next_input_pts` を新 timescale に **切り上げ (`div_ceil`)** で再スケールする (`src/encoder.rs:411-424`)。この切り上げを採用した理由はコメントで「切り捨てだと `new_timescale < old_timescale` 時に rescaled が潰れて逆行し得る」と明示されている。しかし、それを **回帰テスト** で押さえる検証ケースが現状無い。

既存の `reconfigure_rescales_next_input_pts_on_frame_rate_change` (`src/encoder.rs:1593-1622` 内部テスト) は 30000/1001 → 60 の `new_timescale > old_timescale` 方向だけを確認しており、切り下げ方向 (`new_timescale < old_timescale`) は未検証。将来誰かが `div_ceil` を `/` に書き戻しても、現テストではすり抜ける。

## 優先度根拠

- 切り上げの逆行防止効果は公開 API の品質保証として重要 (PTS 単調性は WebRTC 受信側の前提)
- 現状すり抜ける可能性があるが、即座のバグではないので Medium
- 切り上げを採用した設計判断 (`src/encoder.rs:407-408` のコメント参照) を保護するテストとして必須

## 現状

`src/encoder.rs:411-424` の再スケール本体:

```rust
let rescaled_next_input_pts = if let Some(fps) = params.expected_frame_rate {
    let old_timescale = self.config.fps_numerator as u128;
    let new_timescale = fps as u128;
    let old_pts = self.next_input_pts as u128;
    let rescaled = (old_pts * new_timescale).div_ceil(old_timescale);
    if rescaled > i64::MAX as u128 {
        return Err(Error::LimitExceeded {
            reason: "rescaled presentation timestamp overflow",
        });
    }
    Some(rescaled as i64)
} else {
    None
};
```

`next_input_pts` は 0 開始で加算しかされない非負値なので `u128` で計算し、上限のみを `i64::MAX` と比較している。

`new_timescale < old_timescale` のときに切り捨て (`/`) を使うと、`old_pts > 0` の小さい値で `rescaled == 0` になり、直前出力フレームより小さい PTS になって逆行する。それを切り上げで防いでいる。

既存テスト網羅範囲:

- `reconfigure_rescales_next_input_pts_on_frame_rate_change`: 30000/1001 → 60 (= `new_timescale > old_timescale`)
- `reconfigure_preserves_next_input_pts_when_only_bitrate_changes`: fps 不変 (再スケール経路を通らない)
- `reconfigure_overflows_when_rescaled_pts_exceeds_i64_max`: overflow ケース (`new_timescale > old_timescale` 方向の極大値)

切り下げ方向 (`new_timescale < old_timescale`) のテストが完全に欠落。

## 設計方針

`src/encoder.rs` 内部の `#[cfg(test)] mod tests` に新しいテストを追加する。内部テストにするのは `next_input_pts` (private フィールド) を直接読む必要があるため。

検証ケース例:

- 初期 `fps_numerator = 60`、`fps_denominator = 1`
- 1 フレーム encode で `next_input_pts = 1` に進める (または `encoder.next_input_pts = 1` を直接代入してもよい)
- `reconfigure(expected_frame_rate: Some(30))` を呼ぶ
- 期待: `next_input_pts == 1` (`ceil(1 * 30 / 60) = ceil(0.5) = 1`、切り捨てだと 0 になる)
- `encoder.config().fps_numerator == 30`、`encoder.config().fps_denominator == 1` の正規化も確認

可能なら、もうひとつ「`old_pts == 0` の場合は再スケール後も `0` のまま」というケースも追加して、ゼロ境界の挙動を保護する。

## 完了条件

- `src/encoder.rs` 内部 `tests` mod に切り下げ方向の再スケール検証テストが追加されている
- テスト名は `reconfigure_rescales_next_input_pts_ceils_when_new_timescale_smaller` のように切り上げ挙動が分かる名前にする
- `div_ceil` を `/` に書き換えると当該テストが失敗する (実装変更で確認)
- `cargo test --lib` で当該テストが通る
- `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` が通る

## 解決方法

```rust
#[test]
fn reconfigure_rescales_next_input_pts_ceils_when_new_timescale_smaller() -> Result<(), Error> {
    // 60 → 30 の切り下げで `next_input_pts = 1` が 0 に潰れず 1 のまま保たれることを確認する。
    let mut config = base_encoder_config();
    config.fps_numerator = 60;
    config.fps_denominator = 1;
    let mut encoder = Encoder::new(config, noop_handler())?;

    encoder.next_input_pts = 1;

    encoder.reconfigure(ReconfigureParams {
        expected_frame_rate: Some(30),
        ..Default::default()
    })?;

    assert_eq!(encoder.config().fps_numerator, 30);
    assert_eq!(encoder.config().fps_denominator, 1);
    // ceil(1 * 30 / 60) = ceil(0.5) = 1
    assert_eq!(encoder.next_input_pts, 1);
    Ok(())
}
```
