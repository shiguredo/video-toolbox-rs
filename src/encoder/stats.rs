//! エンコーダーの統計値

use crate::stats::{Counter, Gauge};

/// エンコーダーの統計値
///
/// [`crate::Encoder::stats`] が返す共有統計値である。値はエンコーダーを操作する
/// スレッドと、コールバックを実行する Video Toolbox のコールバックスレッドの
/// 両方から更新されるため、各フィールドはスレッドセーフな [`Counter`] / [`Gauge`]
/// で保持する。
///
/// `clone()` は各フィールドの現在値を個別にコピーするため、フィールド間の一貫性は
/// 保証されない。
#[derive(Debug, Clone, Default)]
pub struct EncoderStats {
    /// [`crate::Encoder::encode`] / [`crate::Encoder::encode_pixel_buffer`] が
    /// `VTCompressionSessionEncodeFrame` に受理された通算回数
    ///
    /// 送信前にエラーになったフレーム (PTS オーバーフロー、ピクセルフォーマット不一致、
    /// 入力データ長不足など) は計上しない。`VTCompressionSessionEncodeFrame` 自体が
    /// 失敗した場合も計上しない。
    pub total_encode_count: Counter,

    /// [`crate::EncodeHandler::on_encoded`] に `Ok` を渡した通算回数
    ///
    /// フレームドロップなどで出力データが得られなかった場合は `Err` になるため、
    /// このカウンターは増えない ([`EncoderStats::total_error_count`] が増える)。
    pub total_output_frame_count: Counter,

    /// [`crate::EncodeHandler::on_encoded`] に `Err` を渡した通算回数
    ///
    /// `VTCompressionSessionEncodeFrame` が受理したフレームは、フレームドロップを
    /// 含めて必ず 1 回コールバックが呼ばれる。出力データを組み立てられなかった場合は
    /// エラーとして計上し、正常に出力できた場合だけ
    /// [`EncoderStats::total_output_frame_count`] に計上する。
    pub total_error_count: Counter,

    /// [`crate::Encoder::reconfigure`] が `VTSessionSetProperties` に成功した通算回数
    ///
    /// 更新対象が無い no-op と、`VTSessionSetProperties` が失敗した場合は計上しない。
    pub total_reconfigure_count: Counter,

    /// `VTCompressionSessionEncodeFrame` に受理されたが、まだ出力コールバックが
    /// ユーザーデータを回収していないフレーム数の現在値
    ///
    /// 送信の直前に増やし、出力コールバックがユーザーデータを回収した時点
    /// (ユーザーハンドラーの実行前) に減らす。`VTCompressionSessionEncodeFrame` が
    /// 失敗した場合は増やさない。フレームドロップでもコールバックは呼ばれるため、
    /// [`crate::Encoder::finish`] の完了後に 0 に戻る。
    ///
    /// この値は Video Toolbox が処理中のフレーム数であり、利用側が上限を決めて
    /// [`crate::Encoder::finish`] を挟む間隔を判断するために使う。Video Toolbox が
    /// 満杯を起こさない上限値は公開していないため、上限は利用側で決める。
    pub in_flight_frames: Gauge,
}
