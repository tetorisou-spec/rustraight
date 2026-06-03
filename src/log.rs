// ── ログマクロ (デバッグビルドのみ出力) ──────────────────────────────────────

/// デバッグビルドのみ標準エラーに情報ログを出力する。
///
/// ```ignore
/// log_info!("スプライトをロードしました: '{}'", path);
/// ```
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if ::std::cfg!(debug_assertions) {
            ::std::eprintln!("[情報] {}", ::std::format_args!($($arg)*));
        }
    };
}

/// デバッグビルドのみ標準エラーに警告ログを出力する。
///
/// ```ignore
/// log_warn!("フォントが見つかりません: '{}'", path);
/// ```
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if ::std::cfg!(debug_assertions) {
            ::std::eprintln!("[警告] {}", ::std::format_args!($($arg)*));
        }
    };
}

/// デバッグビルドのみ標準エラーにエラーログを出力する。
///
/// ```ignore
/// log_error!("サーフェスエラー: {}", e);
/// ```
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if ::std::cfg!(debug_assertions) {
            ::std::eprintln!("[エラー] {}", ::std::format_args!($($arg)*));
        }
    };
}
