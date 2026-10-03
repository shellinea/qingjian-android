//! Android 的字体文件清单：系统字体统一在 `/system/fonts`，具体文件随 Android 版本与厂商定制而变，
//! 所以每个角色给几个候选；`FontLibrary::load` 对不存在的文件直接跳过，多列无害。
//!
//! 真机上字体是否齐取决于系统镜像：AOSP 与主流厂商都带 Roboto 与 NotoSansCJK，emoji 是 NotoColorEmoji。

use std::path::PathBuf;

const SYSTEM_FONTS: &str = "/system/fonts";

/// 界面字体：Roboto 是 AOSP 默认；厂商换过 UI 字体时用 Noto Sans / Droid Sans 兜底。
pub(super) fn ui_fonts() -> Vec<PathBuf> {
    [
        "Roboto-Regular.ttf",
        "NotoSans-Regular.ttf",
        "DroidSans.ttf",
        "DroidSansFallback.ttf",
    ]
    .into_iter()
    .map(in_system_fonts)
    .collect()
}

/// 汉字字体：AOSP 里中日韩共用一份 NotoSansCJK 合集，部分机型把简体拆成 NotoSansSC。
/// 按 locale 决定优先顺序，日文排后面当汉语回退。
pub(super) fn script_fonts(locale: &str) -> Vec<PathBuf> {
    let mut names: Vec<&str> = Vec::new();
    if locale.starts_with("ja") {
        names.push("NotoSansCJK-Regular.ttc");
        names.push("NotoSerifCJK-Regular.ttc");
    } else if locale.starts_with("zh-TW") || locale.starts_with("zh-HK") {
        names.push("NotoSansCJK-Regular.ttc");
    } else {
        names.push("NotoSansSC-Regular.otf");
        names.push("NotoSansCJK-Regular.ttc");
    }
    names.push("DroidSansFallback.ttf");
    names.into_iter().map(in_system_fonts).collect()
}

pub(super) fn emoji_fonts() -> Vec<PathBuf> {
    ["NotoColorEmoji.ttf", "NotoColorEmojiCompat.ttf"]
        .into_iter()
        .map(in_system_fonts)
        .collect()
}

fn in_system_fonts(name: &str) -> PathBuf {
    PathBuf::from(format!("{SYSTEM_FONTS}/{name}"))
}
