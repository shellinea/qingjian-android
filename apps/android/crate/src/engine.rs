//! Android 侧的引擎包装：装配数据、组帧、上屏。
//!
//! 与 macOS 壳一样是**进程内直接调用**（`InputMethodService` 与 Engine 同在输入法进程），
//! 不走 Windows 那套「DLL → 命名管道 → Server」的 IPC：
//! - 分页与高亮都在壳里（`CandidateLayout` 只切页，不持有页码），这里保存最近一次
//!   recompose 的结果，`frame(page)` 时按页码切；
//! - 按键的键码→字符映射、拼音行进应用（`commitText`）都在 Kotlin 侧，与 macOS 壳
//!   「Core 不认识 KeyboardEvent」的边界一致；
//! - 上屏走 `Engine::commit`，拼音行原样上屏走 `take_raw`。

use std::path::{Path, PathBuf};

use qingjian_core::{Candidate, CandidateLayout, Engine, Language, Query};
use qingjian_dictionary::Dictionary;
use qingjian_learning::FrequencyLearner;
use qingjian_lm::BigramModel;
use qingjian_platform::protocol::{Frame, PreeditSegment};
use qingjian_translate::{Glossary, LayeredTranslator, PersonalGlossary};

/// 装配引擎需要的数据路径。全部由 Kotlin 侧在第一启动完成解压后给出；
/// 除 `dict` 外都可缺，缺了就退化运行（没有释义表 = 候选旁没有译词）。
pub struct DataPaths {
    /// `data/generated/dict.qj`，唯一必需。
    pub dict: PathBuf,
    /// `data/generated/lm.qj`：bigram 语言模型，没有就退化成一元词频整句。
    pub lm: Option<PathBuf>,
    /// `data/generated/glossary-en.qj`（学习语言对应的那张），有就在候选旁显示译词。
    pub glossary: Option<PathBuf>,
    /// `assets/english.tsv`：英文词表，没有就不出英文候选。
    pub english: Option<PathBuf>,
    /// `Context.getFilesDir()`：学习数据、统计、词汇记录的落盘位置。
    pub data_dir: Option<PathBuf>,
}

/// MVP 只接一种学习语言：英语。配置接线后改成跟 `config.general.learning_language` 走。
const LEARNING_LANGUAGE: Language = Language::English;

pub struct AndroidEngine {
    engine: Engine,
    /// 最近一次 recompose 的候选排布（包含本地候选的完整排布）。
    layout: CandidateLayout,
    /// 拼音行分段（Core 的 `MarkedSegment` 经协议层镜像转换而来）。
    preedit: Vec<PreeditSegment>,
    /// 光标在拼音行里的 char 位置。
    cursor: usize,
    page_size: usize,
    /// 当前高亮的全局下标；翻页键由 Kotlin 侧换算成 page 传进来。
    highlight: usize,
    data_dir: Option<PathBuf>,
}

impl AndroidEngine {
    /// 装配引擎。学习数据读不了（文件损坏 / 无权限）只退回内存态，输入法照常启动。
    pub fn new(paths: &DataPaths) -> Result<Self, String> {
        let dictionary = Dictionary::from_path(&paths.dict).map_err(|e| e.to_string())?;
        let entries = dictionary.len();

        let mut engine = Engine::new(dictionary);

        // 学习数据与统计：没有数据目录就只在内存里学（输入法仍可用，只是学不到东西）。
        if let Some(dir) = &paths.data_dir {
            engine = engine
                .with_learner(Box::new(load_learner(dir)))
                .with_usage_meter(Box::new(qingjian_learning::UsageStats::open(
                    dir.join("usage.tsv"),
                )))
                .with_vocabulary_tracker(Box::new(qingjian_learning::VocabularyBook::open(
                    dir.join("user-vocab.tsv"),
                )));
        }

        // 译词：随包释义表叠上用户目录的个人释义表。
        if let Some(path) = &paths.glossary {
            match Glossary::from_path(LEARNING_LANGUAGE, path) {
                Ok(bundled) => {
                    let personal = match &paths.data_dir {
                        Some(dir) => PersonalGlossary::open(
                            LEARNING_LANGUAGE,
                            dir.join(format!("user-glossary-{}.tsv", LEARNING_LANGUAGE.code())),
                        ),
                        None => PersonalGlossary::in_memory(LEARNING_LANGUAGE),
                    };
                    tracing::info!(glosses = bundled.len(), "随包释义表已加载");
                    engine = engine.with_translator(Box::new(LayeredTranslator::new(bundled, personal)));
                }
                Err(error) => tracing::warn!(%error, "释义表加载失败，候选旁不显示译词"),
            }
        }

        // 语言模型缺失是正常情况，不是错误。
        if let Some(path) = &paths.lm {
            match BigramModel::from_path(path) {
                Ok(model) => {
                    tracing::info!(words = model.word_count(), "语言模型已加载");
                    engine = engine.with_language_model(Box::new(model));
                }
                Err(error) => tracing::warn!(%error, "语言模型加载失败，退化为词频整句"),
            }
        }

        tracing::info!(entries, "Android 引擎装配完成");

        let mut engine = Self {
            engine,
            layout: CandidateLayout::new(Vec::new(), 9, 0),
            preedit: Vec::new(),
            cursor: 0,
            page_size: 9,
            highlight: 0,
            data_dir: paths.data_dir.clone(),
        };
        engine.recompose();
        Ok(engine)
    }

    /// 敲入一个拼音字母。返回组句状态是否有变化（有变化 Kotlin 才重新取帧）。
    pub fn push(&mut self, c: char) -> bool {
        if !c.is_ascii_alphabetic() && c != '\'' {
            return false;
        }
        self.engine.push(c);
        self.recompose();
        true
    }

    /// 退格。返回是否还有组句内容。
    pub fn backspace(&mut self) -> bool {
        let alive = self.engine.backspace();
        self.recompose();
        alive
    }

    /// 清空当前组句（拼音行保留原样由上层调 `take_raw`）。
    pub fn clear(&mut self) {
        self.engine.clear();
        self.recompose();
    }

    /// 选词上屏。`page` 是当前页码，`index` 是页内下标；返回上屏的文本，没有该候选返回 `None`。
    pub fn select(&mut self, page: usize, index: usize) -> Option<String> {
        let candidates = self.page_candidates(page);
        let candidate = candidates.get(index)?;
        Some(self.engine.commit(candidate))
    }

    /// 拼音原样上屏（Esc / 回车）。
    pub fn take_raw(&mut self) -> String {
        let text = self.engine.take_raw();
        self.recompose();
        text
    }

    /// 取一帧：`page` 由 Kotlin 侧按当前高亮换算。页码越界时夹到最后一页。
    pub fn frame(&self, page: usize) -> Frame {
        let page = page.min(self.layout.pages().saturating_sub(1));
        let items = self.page_candidates(page);
        let mut candidates = qingjian_core::CandidateList { items };
        if !candidates.items.is_empty() {
            self.engine.annotate(&mut candidates);
        }
        Frame {
            preedit: self.preedit.clone(),
            cursor: self.cursor,
            candidates,
            highlight: self.highlight.saturating_sub(page * self.page_size),
            page,
            page_count: self.layout.pages().max(1),
            ..Default::default()
        }
    }

    /// 当前是否在组句。
    pub fn composing(&self) -> bool {
        !self.preedit.is_empty()
    }

    /// 失焦 / 销毁前把学习数据与统计落盘。
    pub fn flush(&mut self) {
        self.engine.flush_learning();
        let _ = self.highlight;
    }

    /// 重新查询、组帧：拼音行 + 候选排布。
    fn recompose(&mut self) {
        self.preedit.clear();
        self.highlight = 0;
        let Ok(query) = self.engine.query() else {
            self.layout = CandidateLayout::new(Vec::new(), self.page_size, 0);
            self.cursor = 0;
            return;
        };
        let (preedit, cursor) = marked_parts(&query);
        self.preedit = preedit;
        self.cursor = cursor;
        self.layout = CandidateLayout::new(
            query.candidates.items.clone(),
            self.page_size,
            0,
        );
    }

    fn page_candidates(&self, page: usize) -> Vec<Candidate> {
        self.layout
            .page(page)
            .into_iter()
            .filter_map(|cell| cell.candidate().cloned())
            .collect()
    }
}

/// 读用户的学习数据。格式坏掉的行学习 crate 自己跳过；真读不了（权限、坏盘）就退回只在内存里学，
/// 输入法照常启动，也不会拿空表覆盖用户的文件 —— 学习数据出问题不能让输入法起不来。
fn load_learner(dir: &Path) -> FrequencyLearner {
    let path = dir.join("user.tsv");
    match FrequencyLearner::from_path(&path) {
        Ok(learner) => learner,
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "学习数据读取失败，本次只在内存里学习");
            FrequencyLearner::default()
        }
    }
}

/// 拼音行分段与光标。双拼原始按键显示等配置项接线后再加。
fn marked_parts(query: &Query) -> (Vec<PreeditSegment>, usize) {
    let preedit = query.marked_segments().iter().map(PreeditSegment::from).collect();
    (preedit, query.marked_cursor())
}

impl Drop for AndroidEngine {
    fn drop(&mut self) {
        self.engine.flush_learning();
    }
}
