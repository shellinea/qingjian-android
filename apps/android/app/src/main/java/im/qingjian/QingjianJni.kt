package im.qingjian

/**
 * JNI 桥。方法名与 Rust 侧 `Java_im_qingjian_QingjianJni_*` 一一对应。
 *
 * 句柄是 Rust `Box<AndroidEngine>` 的裸指针：创建后由 Service 持有，
 * `onDestroy` 里必须调 [destroy]，否则内存泄漏（同时丢掉未落盘的学习数据）。
 */
object QingjianJni {

    init {
        System.loadLibrary("qingjian_android")
    }

    /** 创建引擎。失败抛 RuntimeException（Rust 侧 throw）。 */
    external fun create(
        dict: String,
        lm: String?,
        glossary: String?,
        english: String?,
        dataDir: String?,
    ): Long

    /** 敲一个拼音字母，返回组句状态是否变化。 */
    external fun push(handle: Long, code: Int): Boolean

    /** 退格，返回是否还有组句内容。 */
    external fun backspace(handle: Long): Boolean

    /** 取一帧候选（JSON）。page 越界会夹到最后一页。 */
    external fun frame(handle: Long, page: Int): String?

    /** 选词上屏：page 当前页、index 页内下标；无此候选返回 null。 */
    external fun select(handle: Long, page: Int, index: Int): String?

    /** 拼音原样上屏（Esc / 回车）。 */
    external fun takeRaw(handle: Long): String?

    /** 清空组句。 */
    external fun clear(handle: Long)

    /** 是否在组句。 */
    external fun composing(handle: Long): Boolean

    /** 失焦 / 销毁前落盘。 */
    external fun flush(handle: Long)

    /** 释放引擎。 */
    external fun destroy(handle: Long)
}
