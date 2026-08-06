//! エンコード結果を通知するハンドラートレイトとクロージャラッパー

use crate::{encoder::frame::EncodedFrame, error::Error};

/// エンコード結果を通知するためのハンドラー
///
/// エンコード処理が完了するたびに [`EncodeHandler::on_encoded`] が呼ばれる。
pub trait EncodeHandler: Send + 'static {
    /// ユーザーデータ型
    type UserData: Send + 'static;
    /// エラー型
    type Error: From<Error> + Send + 'static;
    /// エンコード完了時に呼ばれる
    fn on_encoded(&mut self, result: Result<EncodedFrame<Self::UserData>, Self::Error>);
}

/// `FnMut(Result<EncodedFrame<T>, E>)` を [`EncodeHandler`] にするラッパー
pub struct FnEncodeHandler<T, E = Error> {
    f: Box<dyn FnMut(Result<EncodedFrame<T>, E>) + Send + 'static>,
}

impl<T, E> FnEncodeHandler<T, E> {
    /// `FnMut(Result<EncodedFrame<T>, E>)` から [`EncodeHandler`] を構築する
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(Result<EncodedFrame<T>, E>) + Send + 'static,
    {
        Self { f: Box::new(f) }
    }
}

impl<T, E> EncodeHandler for FnEncodeHandler<T, E>
where
    T: Send + 'static,
    E: From<Error> + Send + 'static,
{
    type UserData = T;
    type Error = E;
    fn on_encoded(&mut self, result: Result<EncodedFrame<T>, E>) {
        (self.f)(result);
    }
}
