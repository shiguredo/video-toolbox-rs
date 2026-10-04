//! 統計値取得用のプリミティブ型

use std::sync::atomic::{AtomicU64, Ordering};

/// 単調増加カウンター
///
/// 通算値を保持する。`AtomicU64` の薄いラッパーであり、共有が必要な場合は
/// カウンターを含む構造体 ([`crate::EncoderStats`] / [`crate::DecoderStats`]) を
/// `Arc` で包んで行う。
/// コールバックを実行するスレッドが `inc()` でインクリメントし、利用側が `get()` で読み出す。
///
/// カウンターは純粋な累積値であり、スレッド間の happens-before 関係を要求しないため、
/// すべての操作を relaxed order で行う。
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    /// 0 で初期化したカウンターを生成する
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// 現在の値を読み出す
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// カウンターを 1 増やす
    pub(crate) fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

impl Clone for Counter {
    /// 現在値のコピーを取得する
    ///
    /// 共有には `Arc` を使うため、`clone()` は現在値をコピーした独立の値を返す。
    /// 元のカウンターを以後インクリメントしても、この値は変化しない。
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.get()))
    }
}

/// 現在値を表すゲージ
///
/// 時点値を保持する。`AtomicU64` の薄いラッパーであり、単調増加する通算値
/// ([`Counter`]) と異なり、現在の状態を表す値を増減させる。
/// 「送信済みでまだ出力コールバックが来ていないフレーム数」のような、
/// 複数のスレッドから増減される値の保持に使う。
///
/// ゲージは純粋な時点値であり、スレッド間の happens-before 関係を要求しないため、
/// すべての操作を relaxed order で行う。
#[derive(Debug, Default)]
pub struct Gauge(AtomicU64);

impl Gauge {
    /// 0 で初期化したゲージを生成する
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// 現在の値を読み出す
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// 値を 1 増やす
    pub(crate) fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// 値を 1 減らす
    ///
    /// 減算が 0 を下回る場合は 0 で止める。増減の対応が実装バグや Video Toolbox の
    /// 想定外の挙動で崩れた場合に、`u64` の桁溢れで巨大な値に見えるのを避けるため。
    pub(crate) fn dec(&self) {
        // `AtomicU64::try_update` は 1.95.0 で安定化されたため MSRV 1.93 では使えない。
        // `fetch_update` は 1.99.0 で `try_update` への改名により非推奨となるが、
        // 開発と CI はツールチェーンを MSRV の 1.93 に固定しているため警告にならない。
        // MSRV を 1.95 以上に上げる際は `try_update` に置き換えること。
        let _ = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(1))
            });
    }
}

impl Clone for Gauge {
    /// 現在値のコピーを取得する
    ///
    /// 共有には `Arc` を使うため、`clone()` は現在値をコピーした独立の値を返す。
    /// 元のゲージを以後変更しても、この値は変化しない。
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.get()))
    }
}

#[cfg(test)]
mod tests {
    //! `Counter` / `Gauge` の増減は crate 内専用 API のため `tests/` や `pbt/` からは
    //! 到達できない。`new()` / `get()` / `clone()` だけを対象にした PBT は意味を持たないので、
    //! 増減とコピーの検証はこの単体テストで行う。

    use super::*;

    #[test]
    fn counter_new_is_zero() {
        // 生成直後のカウンターは 0 である
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn counter_inc_increments_by_one() {
        // inc() を呼ぶたびに 1 ずつ増える
        let counter = Counter::new();
        counter.inc();
        counter.inc();
        counter.inc();
        assert_eq!(counter.get(), 3);
    }

    #[test]
    fn counter_clone_is_independent_snapshot() {
        // clone() は現在値のコピーであり、以後の変更は互いに影響しない
        let counter = Counter::new();
        counter.inc();

        let snapshot = counter.clone();

        counter.inc();
        assert_eq!(counter.get(), 2);
        assert_eq!(snapshot.get(), 1);
    }

    #[test]
    fn gauge_new_is_zero() {
        // 生成直後のゲージは 0 である
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0);
    }

    #[test]
    fn gauge_inc_and_dec_update_value() {
        // inc() / dec() で現在値を増減できる
        let gauge = Gauge::new();
        gauge.inc();
        gauge.inc();
        gauge.inc();
        assert_eq!(gauge.get(), 3);
        gauge.dec();
        assert_eq!(gauge.get(), 2);
        gauge.dec();
        gauge.dec();
        assert_eq!(gauge.get(), 0);
    }

    #[test]
    fn gauge_dec_saturates_at_zero() {
        // 0 から dec() しても u64 の桁溢れで巨大値にならない
        let gauge = Gauge::new();
        gauge.dec();
        assert_eq!(gauge.get(), 0);
    }

    #[test]
    fn gauge_clone_is_independent_snapshot() {
        // clone() は現在値のコピーであり、以後の変更は互いに影響しない
        let gauge = Gauge::new();
        gauge.inc();
        gauge.inc();

        let snapshot = gauge.clone();

        gauge.inc();
        gauge.inc();
        assert_eq!(gauge.get(), 4);
        assert_eq!(snapshot.get(), 2);
    }
}
