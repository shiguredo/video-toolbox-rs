//! エンコーダーに渡すフレームデータとエンコード結果のフレーム

/// エンコード結果の時刻
///
/// 時刻は有理数 `value / timescale` 秒で表す。`timescale` はその時刻を生成したときに
/// エンコーダーが入力フレームのタイムスタンプに使っていた値
/// ([`crate::EncoderConfig::fps_numerator`] と同じ値) であり、
/// [`crate::Encoder::reconfigure`] でフレームレートを変更すると以後の時刻では変わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// `timescale` を分母とする分子
    pub value: i64,

    /// 1 秒あたりの目盛り数
    pub timescale: i32,
}

impl Timestamp {
    /// 秒単位の時刻を返す
    ///
    /// `value / timescale` の `f64` 除算の結果である。`timescale` が 0 の場合は
    /// `value` の符号に応じて無限大または `NaN` になる (CoreMedia の `CMTimeGetSeconds` と同じ挙動)。
    pub fn seconds(self) -> f64 {
        self.value as f64 / self.timescale as f64
    }
}

/// ピクチャータイプ
///
/// Video Toolbox はピクチャータイプを直接返さないため、出力サンプルに添付された
/// フレーム種別の情報 (`CMSampleAttachmentKey` 系のキー) から判定できる範囲だけを表す。
/// そのため [`PictureType::P`] と [`PictureType::B`] 以外は、他のバックエンドが返す値と
/// 同じ意味にならない場合がある。各バリアントの説明を参照すること。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PictureType {
    /// P フレーム
    ///
    /// 他のフレームを参照する非同期サンプルを表す。他のフレームを参照しないサンプルは
    /// キーフレーム ([`PictureType::I`]) になるため、P フレームと B フレームの区別は
    /// 他のフレームから参照されているかどうかで行う。
    P,

    /// B フレーム
    ///
    /// 他のフレームを参照し、かつ他のフレームから参照されていない非同期サンプルを表す。
    /// この条件を満たすサンプルは他のフレームの参照先にならないため、提示順序を入れ替えても
    /// 他のフレームに影響しない。`allow_frame_reordering` が `false` の場合は現れない。
    B,

    /// I フレーム
    ///
    /// 他のフレームを参照しない同期サンプルを表す。Video Toolbox からは IDR フレームとの
    /// 区別ができないため、IDR フレームもこのバリアントになる (他のバックエンドの
    /// ピクチャータイプで IDR に相当するフレームを含む)。
    I,

    /// 判定に必要な情報が得られなかったフレーム
    ///
    /// 添付情報がフレーム種別を表す辞書でない場合、または非同期サンプルなのに他のフレームを
    /// 参照しないという矛盾した添付情報を持つ場合に返す。この場合、そのフレームは
    /// キーフレームとして扱わない。
    Unknown,
}

/// エンコードされた映像フレーム (AVCC 形式)
#[derive(Debug)]
pub struct EncodedFrame<T> {
    /// フレームの提示時刻
    ///
    /// 有効な提示時刻が得られなかった場合は `None`。`Some` のときの値は、そのフレームの
    /// 提示順序での時刻であり、エンコードの投入順に並べたものではない。
    /// `allow_frame_reordering` が `true` の場合は、`timestamp` が前後に並ぶフレームより
    /// 小さくなることがある。
    pub timestamp: Option<Timestamp>,

    /// ピクチャータイプ
    ///
    /// キーフレームかどうかは [`PictureType::I`] かどうかで判定する。
    pub picture_type: PictureType,

    /// SPS
    pub sps_list: Vec<Vec<u8>>,

    /// PPS
    pub pps_list: Vec<Vec<u8>>,

    /// VPS (H.265 only)
    pub vps_list: Vec<Vec<u8>>,

    /// 圧縮データ
    pub data: Vec<u8>,

    /// `encode` / `encode_pixel_buffer` 呼び出し時に指定したユーザーデータ
    pub user_data: T,
}

/// エンコーダーに渡すフレームデータ
pub enum FrameData<'a> {
    /// I420 (3 プレーン)
    I420 {
        /// Y プレーン
        y: &'a [u8],
        /// U プレーン
        u: &'a [u8],
        /// V プレーン
        v: &'a [u8],
    },
    /// NV12 (2 プレーン)
    Nv12 {
        /// Y プレーン
        y: &'a [u8],
        /// UV インターリーブプレーン
        uv: &'a [u8],
    },
}
