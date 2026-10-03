package im.qingjian

import android.inputmethodservice.InputMethodService
import android.text.TextUtils
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONObject
import java.io.File

private const val TAG = "QingjianIme"

/**
 * 青简 Android 壳。MVP 目标：能装上、能被系统选为输入法、能打拼音出候选并上屏。
 *
 * 数据流：assets 下的 qingjian/ 目录 → 首启解压到 filesDir → Rust 引擎装配；
 * 按键 → push → frame(JSON) → 刷候选条；选词 → select → commitText 上屏。
 */
class QingjianImeService : InputMethodService() {

    private companion object {
        const val ASSET_SUBDIR = "qingjian"
    }

    private var engine = 0L
    private var page = 0
    private var pageCount = 1
    private var highlight = 0

    private lateinit var candidatesRow: LinearLayout
    private lateinit var keyboardRow: LinearLayout

    /** 解压完成后引擎指针的持有者；失败时保持 0，所有入口判空后直接返回。 */
    override fun onCreate() {
        super.onCreate()
        val files = File(filesDir, ASSET_SUBDIR)
        if (!File(files, "dict.qj").exists()) {
            unpackAssets(files)
        }
        engine = try {
            QingjianJni.create(
                dict = File(files, "dict.qj").absolutePath,
                lm = File(files, "lm.qj").takeIf { it.exists() }?.absolutePath,
                glossary = File(files, "glossary-en.qj").takeIf { it.exists() }?.absolutePath,
                english = File(files, "english.tsv").takeIf { it.exists() }?.absolutePath,
                dataDir = filesDir.absolutePath,
            )
        } catch (e: Exception) {
            Log.e(TAG, "引擎创建失败", e)
            0L
        }
    }

    override fun onCreateInputView(): View {
        Log.i(TAG, "onCreateInputView")
        val view = layoutInflater.inflate(R.layout.view_input, null)
        candidatesRow = view.findViewById(R.id.candidates)
        keyboardRow = view.findViewById(R.id.keyboard)
        try {
            buildKeyboard()
            Log.i(TAG, "buildKeyboard 完成，键盘子 View 数=${keyboardRow.childCount}")
        } catch (e: Throwable) {
            Log.e(TAG, "buildKeyboard 失败", e)
        }
        return view
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        Log.i(TAG, "onStartInput engine=$engine")
        page = 0
        highlight = 0
        refresh()
    }

    /**
     * 退掉全屏提取模式：某些设备（模拟器竖屏 + imeOptions 无 specialFlag）会走
     * fullscreen extract，主输入视图被 mViewVisibility=INVISIBLE 隐藏，只剩 extract view。
     * 我们不需要 extract，强制关掉。
     */
    override fun onEvaluateFullscreenMode(): Boolean = false

    /** 输入视图的显示也要主动请求：否则系统可能判定输入框不需要可见键盘。 */
    override fun onStartInputView(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(attribute, restarting)
        Log.i(TAG, "onStartInputView")
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        super.onFinishInputView(finishingInput)
        if (engine != 0L) QingjianJni.flush(engine)
    }

    override fun onDestroy() {
        if (engine != 0L) {
            QingjianJni.flush(engine)
            QingjianJni.destroy(engine)
            engine = 0L
        }
        super.onDestroy()
    }

    /** 把 assets 下 qingjian/ 目录里的文件解压到 filesDir 下的同名子目录。 */
    private fun unpackAssets(dest: File) {
        dest.mkdirs()
        val entries = assets.list(ASSET_SUBDIR) ?: return
        var copied = 0L
        for (name in entries) {
            val target = File(dest, name)
            if (target.exists()) continue
            try {
                assets.open("$ASSET_SUBDIR/$name").use { input ->
                    target.outputStream().use { output ->
                        input.copyTo(output, 1 shl 20)
                    }
                }
                copied += target.length()
            } catch (e: Exception) {
                Log.e(TAG, "解压 $name 失败", e)
            }
        }
        Log.i(TAG, "随包数据解压完成: ${copied / 1024 / 1024} MB")
    }

    /** 三行字母 + 一行功能键。 */
    private fun buildKeyboard() {
        val rows = listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
        val weight: Float
        for (row in rows) {
            val line = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                // 必须是 MATCH_PARENT 宽：子按钮按 weight 分宽，容器 wrap_content 时 0 宽子 View 会全塌成 0
                layoutParams = LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                )
            }
            for (c in row) {
                line.addView(functionKey(c.toString()) { onLetter(c) })
            }
            keyboardRow.addView(line)
        }

        val functionRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }
        functionRow.addView(functionKey("⌫", 1.2f) { onBackspace() })
        functionRow.addView(functionKey("，") { onPunctuation("，") })
        functionRow.addView(functionKey("空格", 2.4f) { onSpace() })
        functionRow.addView(functionKey("。") { onPunctuation("。") })
        functionRow.addView(functionKey("⏎") { onEnter() })
        keyboardRow.addView(functionRow)
    }

    private fun functionKey(
        label: String,
        weight: Float = 1f,
        onClick: () -> Unit,
    ): Button {
        val density = resources.displayMetrics.density
        return Button(this).apply {
            text = label
            textSize = 17f
            isAllCaps = false
            gravity = Gravity.CENTER
            minWidth = 0
            minimumWidth = 0
            setOnClickListener { onClick() }
            layoutParams = LinearLayout.LayoutParams(
                0,
                (50 * density).toInt(),
                weight,
            )
        }
    }

    private fun onLetter(c: Char) {
        if (engine == 0L) return
        if (QingjianJni.push(engine, c.code)) {
            page = 0
            highlight = 0
            refresh()
        }
    }

    private fun onBackspace() {
        if (engine == 0L) return
        if (QingjianJni.backspace(engine)) {
            page = 0
            refresh()
        } else {
            // 组句空了：退格交给应用删字符
            currentInputConnection?.deleteSurroundingText(1, 0)
            refresh()
        }
    }

    private fun onSpace() {
        if (engine == 0L) return
        if (QingjianJni.composing(engine)) {
            select(highlight)
        } else {
            commit(" ")
        }
    }

    private fun onEnter() {
        if (engine == 0L) return
        if (QingjianJni.composing(engine)) {
            QingjianJni.takeRaw(engine)?.let { raw ->
                commit(raw)
                refresh()
            }
        } else {
            sendDefaultEditorAction(true)
        }
    }

    private fun onPunctuation(text: String) {
        commit(text)
        refresh()
    }

    private fun select(index: Int) {
        val text = QingjianJni.select(engine, page, index)
        if (!text.isNullOrEmpty()) {
            commit(text)
            QingjianJni.clear(engine)
        }
        // clear 之后 Rust 侧 preedit 已空，这一帧会是空帧：候选条应当收起。
        // 不再调用 refresh() —— clear 已置空组句，UI 直接清空容器即可。
        candidatesRow.removeAllViews()
    }

    private fun commit(text: String) {
        currentInputConnection?.commitText(text, 1)
    }

    /** 从 JSON 帧刷候选条。 */
    private fun refresh() {
        if (engine == 0L || !::candidatesRow.isInitialized) return
        val json = QingjianJni.frame(engine, page) ?: run {
            candidatesRow.removeAllViews()
            return
        }
        val frame = try {
            JSONObject(json)
        } catch (e: Exception) {
            Log.e(TAG, "帧解析失败", e)
            return
        }
        val items = frame.optJSONObject("candidates")?.optJSONArray("items")
        pageCount = frame.optInt("page_count", 1)
        highlight = frame.optInt("highlight", 0)

        candidatesRow.removeAllViews()
        if (items == null || items.length() == 0) {
            val preedit = frame.optJSONArray("preedit")
            val text = preedit?.let { segments ->
                (0 until segments.length())
                    .joinToString("") { segments.getJSONObject(it).optString("text") }
            }
            if (!text.isNullOrEmpty()) {
                candidatesRow.addView(candidateView("拼音: $text（无候选）", index = 0))
            }
            return
        }
        for (i in 0 until items.length()) {
            candidatesRow.addView(candidateView(labelOf(items.getJSONObject(i)), index = i))
        }
        if (page + 1 < pageCount) {
            candidatesRow.addView(
                TextView(this).apply {
                    text = "翻页↓"
                    textSize = 16f
                    gravity = Gravity.CENTER
                    val density = resources.displayMetrics.density
                    setPadding((14 * density).toInt(), 0, (14 * density).toInt(), 0)
                    setOnClickListener {
                        page += 1
                        refresh()
                    }
                },
            )
        }
    }

    private fun candidateView(label: String, index: Int): TextView {
        val density = resources.displayMetrics.density
        return TextView(this).apply {
            text = label
            textSize = 17f
            gravity = Gravity.CENTER
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setPadding((14 * density).toInt(), 0, (14 * density).toInt(), 0)
            tag = index
            setOnClickListener { select(index) }
        }
    }

    /** 候选文案：主词 + 词性与译文。 */
    private fun labelOf(candidate: JSONObject): String {
        val text = candidate.optString("text")
        val senses = sensesOf(candidate)
        return if (senses.isEmpty()) text else "$text $senses"
    }

    private fun sensesOf(candidate: JSONObject): String {
        val translation = candidate.optJSONObject("translation") ?: return ""
        val senses = translation.optJSONArray("senses") ?: return ""
        val out = mutableListOf<String>()
        for (i in 0 until senses.length()) {
            val sense = senses.getJSONObject(i)
            val pos = sense.optString("part_of_speech")
            val meaning = sense.optString("text")
            if (meaning.isNotEmpty()) {
                out.add(if (pos.isNotEmpty()) "$pos. $meaning" else meaning)
            }
        }
        return out.joinToString(" / ")
    }
}