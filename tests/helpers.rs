//! テスト間で共有するヘルパー

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

/// テストのログ出力を収集する writer (tracing-subscriber の `with_writer` 用)
///
/// コールバックの panic 捕捉時のログ出力を検証するテストで使う。
#[derive(Clone)]
pub struct LogCollector(Arc<Mutex<Vec<u8>>>);

impl Write for LogCollector {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("ログ収集バッファの mutex が poison になっている")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// テスト全体で共有するログ収集バッファ
///
/// Video Toolbox のコールバックは別スレッドで実行されるため、スレッドローカルな
/// `tracing::subscriber::with_default` ではログを収集できない。グローバル subscriber を
/// 1 回だけ設定し、全テストが同じバッファを共有する。
///
/// 注意: バッファはプロセス全体で 1 つであり、各テストが `clear()` してから検証する。
/// 並列実行中に他テストのログが混入し得るため、ログ検証テストの実行は
/// `--test-threads=1` を前提とする (CI は `cargo test --workspace -- --test-threads=1`)。
/// 検証は `contains` による部分一致のため、他テストのログが混入しても誤検出にはならない。
static GLOBAL_LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

/// ログ収集バッファを初期化し、グローバル subscriber を設定する (初回のみ)
///
/// 各テストは戻り値のバッファを `clear()` してから実行し、検証後に内容を確認する。
pub fn init_global_log_collector() -> Arc<Mutex<Vec<u8>>> {
    GLOBAL_LOGS
        .get_or_init(|| {
            let logs = Arc::new(Mutex::new(Vec::new()));
            let log_writer = LogCollector(Arc::clone(&logs));
            let subscriber = tracing_subscriber::fmt()
                .with_writer(move || log_writer.clone())
                .with_ansi(false)
                .finish();
            // 既に設定済みの場合は Err が返るが、設定済みの subscriber が使われるため無視してよい
            let _ = tracing::subscriber::set_global_default(subscriber);
            logs
        })
        .clone()
}

/// ログ収集バッファを空にして、ログ検証テストの開始状態にする
pub fn clear_logs(logs: &Arc<Mutex<Vec<u8>>>) {
    logs.lock()
        .expect("ログ収集バッファの mutex が poison になっている")
        .clear();
}

/// ログ収集バッファの内容を文字列として取り出す
pub fn take_logs(logs: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(
        logs.lock()
            .expect("ログ収集バッファの mutex が poison になっている")
            .clone(),
    )
    .expect("ログは UTF-8 でなければならない")
}

/// ログに指定した文字列が含まれることを検証する
pub fn assert_log_contains(log: &str, expected: &str) {
    assert!(
        log.contains(expected),
        "ログに「{expected}」が含まれていること: {log}"
    );
}
