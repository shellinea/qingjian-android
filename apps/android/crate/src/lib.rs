//! Android 输入法壳的 Rust 侧：JNI 暴露层。
//!
//! 命名约定：Kotlin 侧 `im.qingjian.QingjianJni`（object）里的 `external fun`，
//! 对应 `Java_im_qingjian_QingjianJni_<方法名>`。Engine 用裸指针经 `jlong` 传，
//! 生命周期由 Kotlin 侧持有、`destroy` 释放；指针为 0 表示创建失败，所有入口都判空。
//!
//! jni 0.22 的写法：native 方法的 env 形参是 `EnvUnowned`（`JNIEnv` 的弃用别名），
//! 一切操作都在 `with_env(|env: &mut Env| ...)` 闭包里做，最后用
//! `resolve::<ThrowRuntimeExAndDefault>()` 收尾 —— 闭包 panic 或被 JVM
//! 中断也会被兜住，返回 0 / null 并向 Kotlin 抛 RuntimeException。
//!
//! 线程：InputMethodService 的回调都在主线程，Engine 不是 `Sync`，所以所有 JNI
//! 入口都只在主线程被调用，内部不做额外同步。

#![allow(clippy::missing_safety_doc)]

use std::path::PathBuf;
use std::sync::Once;

use jni::errors::{Error, Result, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use jni::{Env, EnvUnowned};

mod engine;

use engine::{AndroidEngine, DataPaths};

/// Rust 的 tracing 桥到 Android logcat。不加 android_logger 依赖：logcat 也认
/// `__android_log_write`，但要额外绑 JNI；这里直接用 tracing 的 Subscriber 太重，
/// 改用最省事的：在 lib 初始化时装一个基于 `eprintln!` → stderr 的 layer，Android
/// 上 stderr 会落到 logcat 的 `stderr` 缓冲，`adb logcat` 能直接看到。
static LOG_INIT: Once = Once::new();

fn init_logging() {
    LOG_INIT.call_once(|| {
        use tracing_subscriber::fmt;
        fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .init();
    });
}

/// 引擎句柄转回引用；0 给 `None`（上层按失败处理）。
fn engine_mut(handle: jlong) -> Option<&'static mut AndroidEngine> {
    if handle == 0 {
        return None;
    }
    unsafe { (handle as *mut AndroidEngine).as_mut() }
}

/// 读一个可能为 null 的字符串参数。
fn string_arg(env: &mut Env, value: &JString) -> Option<String> {
    if value.as_raw().is_null() {
        return None;
    }
    // get_string 在 0.22 已弃用，但 mutf8_chars 需要自行处理 MUTF8 解码；
    // 这里内容短且是本地路径，弃用 API 更直接
    #[allow(deprecated)]
    let chars = env.get_string(value).ok()?;
    Some(chars.to_str().into_owned())
}

/// 把 Rust 字符串交回 JVM。失败（极少见：OOM）就抛异常、返回 null。
fn to_jstring(env: &mut Env, text: &str) -> Result<jstring> {
    let text = env.new_string(text)?;
    Ok(text.into_raw())
}

/// 创建引擎：`QingjianJni.create(dict, lm, glossary, english, dataDir)`。
/// 返回指针；失败返回 0 并向 Kotlin 抛异常。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_create<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    dict: JString<'local>,
    lm: JString<'local>,
    glossary: JString<'local>,
    english: JString<'local>,
    data_dir: JString<'local>,
) -> jlong {
    env.with_env(|env: &mut Env| -> Result<jlong> {
        init_logging();
        let Some(dict) = string_arg(env, &dict) else {
            return Err(Error::NullPtr("dict 路径为空"));
        };
        let paths = DataPaths {
            dict: PathBuf::from(dict),
            lm: string_arg(env, &lm).map(PathBuf::from),
            glossary: string_arg(env, &glossary).map(PathBuf::from),
            english: string_arg(env, &english).map(PathBuf::from),
            data_dir: string_arg(env, &data_dir).map(PathBuf::from),
        };
        let engine = match AndroidEngine::new(&paths) {
            Ok(engine) => engine,
            Err(message) => {
                let _ = env.throw(format!("qingjian engine init failed: {message}"));
                return Err(Error::JavaException);
            }
        };
        Ok(Box::into_raw(Box::new(engine)) as jlong)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 敲一个拼音字母。返回组句状态是否变了（Kotlin 据此决定要不要重新取帧）。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_push<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    code: jint,
) -> jni::sys::jboolean {
    env.with_env(|_env: &mut Env| -> Result<jni::sys::jboolean> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(false);
        };
        let Some(c) = char::from_u32(code as u32) else {
            return Ok(false);
        };
        Ok(engine.push(c))
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 退格。返回是否还有组句内容（没有就由 Kotlin 把退格交给应用）。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_backspace<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jni::sys::jboolean {
    env.with_env(|_env: &mut Env| -> Result<jni::sys::jboolean> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(false);
        };
        Ok(engine.backspace())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 取一帧候选（JSON 的 `Frame`）。`page` 越界会夹到最后一页。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_frame<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    page: jint,
) -> jstring {
    env.with_env(|env: &mut Env| -> Result<jstring> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(std::ptr::null_mut());
        };
        let frame = engine.frame(page.max(0) as usize);
        let json = match serde_json::to_string(&frame) {
            Ok(json) => json,
            Err(e) => {
                let _ = env.throw(format!("qingjian frame serialize failed: {e}"));
                return Err(Error::JavaException);
            }
        };
        to_jstring(env, &json)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 选词上屏：`page` 当前页、`index` 页内下标。返回上屏文本；没有该候选返回 null。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_select<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    page: jint,
    index: jint,
) -> jstring {
    env.with_env(|env: &mut Env| -> Result<jstring> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(std::ptr::null_mut());
        };
        let Some(text) = engine.select(page.max(0) as usize, index.max(0) as usize) else {
            return Ok(std::ptr::null_mut());
        };
        to_jstring(env, &text)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 拼音原样上屏（Esc / 回车）。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_takeRaw<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    env.with_env(|env: &mut Env| -> Result<jstring> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(std::ptr::null_mut());
        };
        to_jstring(env, &engine.take_raw())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 清空组句（上屏后继续打下一个词）。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_clear<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    env.with_env(|_env: &mut Env| -> Result<()> {
        if let Some(engine) = engine_mut(handle) {
            engine.clear();
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 是否还在组句（决定候选条的显隐）。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_composing<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jni::sys::jboolean {
    env.with_env(|_env: &mut Env| -> Result<jni::sys::jboolean> {
        let Some(engine) = engine_mut(handle) else {
            return Ok(false);
        };
        Ok(engine.composing())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 失焦 / 销毁前把学习数据与统计落盘。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_flush<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    env.with_env(|_env: &mut Env| -> Result<()> {
        if let Some(engine) = engine_mut(handle) {
            engine.flush();
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// 释放引擎。
#[no_mangle]
pub extern "system" fn Java_im_qingjian_QingjianJni_destroy<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    env.with_env(|_env: &mut Env| -> Result<()> {
        if handle != 0 {
            unsafe { drop(Box::from_raw(handle as *mut AndroidEngine)) };
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}
