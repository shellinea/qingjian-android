# Android 移植

本页记录 Android 壳的移植状态、环境依赖与决策，作为跨会话、跨机器的进度锚点。方案骨架见 `docs/plan/roadmap.md` 的 Phase 5。

状态：**Phase A 完成（2026-10-03）**，Phase B 未开始。

## 为什么做

上游路线图里没有移动端（issue #235 以 not_planned 关闭，作者的选择是「移动端短期不在路线图上，维持缩小覆盖即可」）。本分支是下游移植：目标是先把引擎栈在 Android 目标下的编译可见性打通，再补一个 `InputMethodService` 壳。

## 阶段

| 阶段 | 内容 | 状态 |
|---|---|---|
| A | 交叉编译打通 + TLS 目标门控 | 完成 |
| B | 最小可用 IME（打拼音出候选、选词上屏、数据落盘） | 未开始 |
| C | 自绘候选窗 + 译词 + 生词标记 | 未开始 |
| D | 学习数据、最小设置页、按 ABI 拆包 | 未开始 |

## Phase A 做了什么

六个文件，+81 −5：

- `Cargo.toml`（workspace）：`async-openai` 去掉写死的 `native-tls` feature。
- `crates/qingjian-predict/Cargo.toml`：`async-openai` 与 `reqwest` 移入 `[target.'cfg(...)'.dependencies]` 互补两分支。
- `crates/qingjian-update/Cargo.toml`：`reqwest` 同样门控；`system-proxy` 在两条分支都显式保留。
- `tools/gloss-gen/Cargo.toml`：`async-openai` 同样门控。
- `crates/qingjian-render/src/fonts/mod.rs` 与新增 `.../fonts/android.rs`：补 android 平台分支。

四处 manifest 必须同时改：feature 在同一目标内取并集，漏一处就会把 `native-tls` / `openssl-sys` 重新拉进 Android 构建。

## 验收（2026-10-03）

| 项目 | 结果 |
|---|---|
| `cargo check --target aarch64-linux-android` × 11 个 crate（stable 1.99.0） | 退出码 0 |
| 同上，用仓库 pin 的 1.96.0 | 退出码 0 |
| `--target x86_64-linux-android` × 11 个 crate | 退出码 0 |
| 桌面回归 `-p qingjian-{render,predict,update,platform,core}`（host） | 退出码 0 |
| `cargo tree -p qingjian-update --target x86_64-pc-windows-msvc -i native-tls` | `native-tls → hyper-tls → reqwest → async-openai`（桌面未变） |
| `cargo tree -p qingjian-update --target aarch64-linux-android` | `hyper-rustls → rustls → tokio-rustls`，无 openssl |

11 个 crate：`core`、`dictionary`、`format`、`learning`、`lm`、`neural`、`platform`、`render`、`translate`、`predict`、`update`。其中 `platform` / `predict` / `update` 是 #235 说「在 Android 目标上还没有编译可见性」的三个。

## 本机环境（可重建）

| 项 | 路径 |
|---|---|
| Rust | `D:\DevTools\rust`（`RUSTUP_HOME` / `CARGO_HOME` 都在这里），stable 1.99.0 与 1.96.0，两个 Android target |
| JDK | `D:\DevTools\java\jdk-21.0.12.1+1` |
| Android SDK | `D:\Android\Sdk`（platform-tools、platforms;android-35、build-tools;35.0.0、ndk;30.0.16248370、cmdline-tools/latest） |
| 源码 | `D:\DevTools\qingjian` |

cargo 配置（`D:\DevTools\rust\.cargo\config.toml`）：`[http] check-revoke = false`、`[net] git-fetch-with-cli = true`、crates.io 换 USTC 稀疏索引（国内直连 crates.io 与 github 都不稳）。

交叉编译需要的环境变量（每次新 shell 都要设，或写进脚本）：

```powershell
$ndk = 'D:\Android\Sdk\ndk\30.0.16248370'
$bin = "$ndk\toolchains\llvm\prebuilt\windows-x86_64\bin"
$api = 24
$env:CC_aarch64_linux_android  = "$bin\aarch64-linux-android$api-clang.cmd"
$env:CXX_aarch64_linux_android = "$bin\aarch64-linux-android$api-clang++.cmd"
$env:AR_aarch64_linux_android  = "$bin\llvm-ar.exe"
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "$bin\aarch64-linux-android$api-clang.cmd"
$env:CMAKE_TOOLCHAIN_FILE = "$ndk\build\cmake\android.toolchain.cmake"
```

（`x86_64-linux-android` 同理把前缀换掉。）`CMAKE_TOOLCHAIN_FILE` 是给 `aws-lc-sys` 的；`CC_*` 是给 `onig_sys`（经 `tokenizers`）的。

## Phase B 前置清单

1. `crates/qingjian-platform/src/dirs.rs`：加 android 分支。现在 `user_dir()` 读 `APPDATA`、`log_dir()` 读 `LOCALAPPDATA`，Android 上两者恒为 `None`，函数直接返回 `None` —— 学习数据、统计、输入日志、配置会**静默全关**，是最容易漏的一处。
2. `crates/qingjian-platform/src/resources.rs:13 bundled_root()`：`current_exe()` 探测在 Android 上指向 APK 里的 so，不成立；改成壳注入。`apps/linux/server/src/paths.rs:22` 的 `QINGJIAN_RESOURCES` 环境变量模式是现成先例。
3. `crates/qingjian-platform/src/config/apps.rs:80-89`：补 android 默认应用名单（现在会落到 Linux 的 fcitx5 名单）。
4. 新建 `apps/android/`：`InputMethodService` 子类 + JNI 桥 + 候选 View。数据从 APK assets 首启解压到 `filesDir` 再 mmap，目录布局照 `apps/windows/installer/qingjian.iss:80-104`（`<root>/data/generated/...` + `<root>/assets/...`），这样壳内的相对路径拼接不用改。

## 决策记录

- **TLS 门控写在 consumer 而不是 workspace**：`[workspace.dependencies]` 的 features 对成员是叠加的，写死之后成员无法按目标抵消；而仓库 pin 的 Rust 是 1.96.0（低于能用 `default-features` 覆盖 workspace 值的 1.99）。所以只能四处 consumer 各写 `[target.'cfg(...)'.dependencies]`。
- **桌面行为保持不变**：Windows/macOS 继续走 native-tls（schannel / Security.framework），只有非桌面目标走 rustls。
- **字号回退用候选列表**：Android 系统字体文件随版本与厂商定制而变，`fonts/android.rs` 每个角色列几个候选，不存在的由 `FontLibrary::load` 的 `is_file()` 检查跳过。
- **`cargo check` 不含 `apps/macos`**：那个壳依赖 `objc2` 系列，在非 Apple 目标上必然失败（`objc2` 自己会 `compile_error!`），属于平台壳的正常边界。

## 踩过的坑

- **两个 rustup 进程不能并行装同一 toolchain**：会互相挪走对方的 `.partial`，报 `could not rename` 或 `RecvError` panic。必须串行。
- **git 里的失效代理**：`http.proxy = http://127.0.0.1:7890` 在代理进程不在监听时会让所有 github 访问失败，报错却是 `Could not connect to server`，容易误判成网络不通。清掉配置后直连正常。
- **页面文件**：本机原本没有页面文件（`PagingFiles` 为空），提交上限被锁在物理内存大小；rustup 解压会报 `can't allocate memory`，cargo 编译同样会 OOM。
- **`onig_sys` / `aws-lc-sys` 需要 C 工具链**：不配 `CC_*` 与 `CMAKE_TOOLCHAIN_FILE` 会报 `failed to find tool "clang.exe"`。
