//! 随包资源（词库 / 语言模型 / 释义表 / 等级表 / emoji / 样例）的定位。
//!
//! 两套布局，装机优先、回落开发：
//! - **装机**：资源与可执行文件同级（安装程序把 `data\` / `assets\` 装在 exe 旁）。
//! - **开发**：仓库 `ime/` 目录，exe 在 `ime\target\{debug,release}\` 下，往上三层。
//!
//! 相对写法两套布局一致（如 `data/generated/dict.qj`、`assets/levels`），只有根不同。
//! Windows 的 Server 进程与设置窗口共用；macOS 有自己的 `paths.rs`，不走这里。
//!
//! Android 上两套探测都不成立（`current_exe()` 指向 APK 里的 `.so`），根由壳注入，见 [`set_bundled_root`]：
//! 壳在首启时把 APK 内的 assets 解压到应用私有目录，解压结果保持 `data/` + `assets/` 的布局即可。

use std::path::PathBuf;

#[cfg(not(target_os = "android"))]
use std::path::Path;

#[cfg(target_os = "android")]
use std::sync::OnceLock;

/// Android：随包资源的根（壳解压 assets 得到的目录）。
#[cfg(target_os = "android")]
static BUNDLED_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Android：注入随包资源根。只生效一次；目录下需要有 `data/` 与 `assets/`。
#[cfg(target_os = "android")]
pub fn set_bundled_root(dir: PathBuf) {
    let _ = BUNDLED_ROOT.set(dir);
}

/// 随包资源的根目录：其下有 `data/` 与 `assets/`。装机布局与 exe 同级，否则回落开发布局的仓库根；两处都没有为 `None`。
#[cfg(not(target_os = "android"))]
pub fn bundled_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    if has_resources(exe_dir) {
        return Some(exe_dir.to_path_buf());
    }
    // exe → {debug,release} → target → ime
    let dev_root = exe.ancestors().nth(3)?;
    has_resources(dev_root).then(|| dev_root.to_path_buf())
}

/// 随包资源的根目录：Android 上取壳注入的根，未注入为 `None`。
#[cfg(target_os = "android")]
pub fn bundled_root() -> Option<PathBuf> {
    BUNDLED_ROOT.get().cloned()
}

/// 一个随包资源的完整路径（相对随包根，如 `data/generated/dict.qj`）；根找不到或该路径不存在为 `None`。
pub fn bundled_resource(rel: &str) -> Option<PathBuf> {
    let path = bundled_root()?.join(rel);
    path.exists().then_some(path)
}

/// 一个目录是不是随包根：有 `data` 或 `assets` 子目录就算。Android 的根由壳注入，不走这套探测。
#[cfg(not(target_os = "android"))]
fn has_resources(dir: &Path) -> bool {
    dir.join("data").is_dir() || dir.join("assets").is_dir()
}
