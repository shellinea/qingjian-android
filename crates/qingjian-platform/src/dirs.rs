//! Windows 上用户目录的约定，Server / TSF DLL / 设置程序三处共用：
//!
//! - **数据** `%APPDATA%\Qingjian`：配置、密钥、学习数据、统计、输入日志（漫游目录，跟着账户走）。
//! - **日志** `%LOCALAPPDATA%\Qingjian\logs`：三个进程的运行日志都在这一个目录里，
//!   `server.<日期>.log` / `tsf.<日期>.log` / `settings.<日期>.log`，用户反馈问题时整个目录打包即可。
//!   放本机目录有两个原因：日志本来就是这台机器的东西，不该跟账户漫游；DLL 被加载进
//!   AppContainer 应用（任务栏搜索 / 设置）时写不了漫游目录，Server 启动时会给这个目录授权。
//!
//! 只查环境变量、不碰系统 API，其他平台也能编（拿不到为 `None`）。macOS 有自己的 `paths.rs`，不走这里。
//!
//! Android 没有 `APPDATA` / `LOCALAPPDATA`，两处都由壳（`InputMethodService` / `Application`）
//! 在初始化时用 `Context.getFilesDir()` 注入，见 [`set_app_data_dir`]。注入之前这两个函数返回 `None`，
//! 学习数据、统计、输入日志与配置就只停在内存里。

use std::path::PathBuf;

#[cfg(target_os = "android")]
use std::sync::OnceLock;

/// Android 的应用数据根（`Context.getFilesDir()`），由壳注入。
#[cfg(target_os = "android")]
static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Android：注入应用数据根。只生效一次，重复调用忽略；壳应在装配引擎与学习数据之前调用。
#[cfg(target_os = "android")]
pub fn set_app_data_dir(dir: PathBuf) {
    let _ = APP_DATA_DIR.set(dir);
}

/// Android：当前注入的数据根，未注入为 `None`。
#[cfg(target_os = "android")]
pub fn app_data_dir() -> Option<PathBuf> {
    APP_DATA_DIR.get().cloned()
}

/// 用户数据目录：Windows 上是 `%APPDATA%\Qingjian`，Android 上是壳注入的应用私有目录。
#[cfg(not(target_os = "android"))]
pub fn user_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("Qingjian"))
}

/// 用户数据目录：Android 上是壳注入的应用私有目录（`Context.getFilesDir()`）。
#[cfg(target_os = "android")]
pub fn user_dir() -> Option<PathBuf> {
    app_data_dir()
}

/// 配置文件 `%APPDATA%\Qingjian\config.toml`。
pub fn config_path() -> Option<PathBuf> {
    user_dir().map(|dir| dir.join("config.toml"))
}

/// 运行日志目录：Windows 上是 `%LOCALAPPDATA%\Qingjian\logs`，Android 上是数据根下的 `logs/`。不负责创建。
#[cfg(not(target_os = "android"))]
pub fn log_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Qingjian").join("logs"))
}

/// 运行日志目录：Android 上是数据根下的 `logs/`。不负责创建。
#[cfg(target_os = "android")]
pub fn log_dir() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("logs"))
}
