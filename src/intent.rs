//! Turns a chat message into what the asker wants from the 8-ball.
//!
//! The parser works clause by clause and looks at every "吃" in a clause:
//! the words before it say whether this is a question ("要不要", "晚餐"),
//! and the words after it name the food, the options, or a question word.

use crate::meal::Meal;
use crate::text::{self, Generic, MAX_OPTIONS};

/// Messages longer than this are conversation, not a dinner question.
pub const MAX_MESSAGE_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// "晚餐吃什麼？": pick something.
    Recommend { meal: Option<Meal> },
    /// "要不要吃火鍋？": give a verdict on one food.
    Judge { food: String, meal: Option<Meal> },
    /// "火鍋還是拉麵？": pick one of the offered options.
    Choose {
        options: Vec<String>,
        meal: Option<Meal>,
    },
}

impl Intent {
    pub fn meal(&self) -> Option<Meal> {
        match self {
            Intent::Recommend { meal }
            | Intent::Judge { meal, .. }
            | Intent::Choose { meal, .. } => *meal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Recommend,
    Judge(String),
    Choose(Vec<String>),
}

impl Kind {
    fn into_intent(self, meal: Option<Meal>) -> Intent {
        match self {
            Kind::Recommend => Intent::Recommend { meal },
            Kind::Judge(food) => Intent::Judge { food, meal },
            Kind::Choose(options) => Intent::Choose { options, meal },
        }
    }
}

const CLAUSE_SEPARATORS: &[char] = &[
    '，', ',', '。', '.', '！', '!', '？', '?', '\n', '；', ';', '~', '～', '…', '：', ':',
];

/// Time words that mean the meal is in the past, so nobody is asking.
const PAST_MARKERS: &[&str] = &[
    "昨天",
    "昨晚",
    "前天",
    "上次",
    "上週",
    "上禮拜",
    "上星期",
    "剛剛",
    "剛才",
];

/// Words right before 吃 that make it a description, not a question:
/// "你在吃什麼" (progressive), "好吃", "難吃", "小吃", "你平常都吃什麼" (habit).
const NON_QUESTION_BEFORE: &[&str] = &["在", "好", "難", "小", "常", "都"];

/// Words right after 吃 that mean the eating is done or is about capacity.
#[rustfmt::skip]
const NON_QUESTION_AFTER: &[&str] = &[
    "了", "過", "飽", "完", "掉", "到了", "下去", "下肚", "得", "光", "不下", "不完", "不飽",
];

/// "要不要吃X" style questions always ask for a verdict.
const CHOICE_QUESTIONS: &[&str] = &["要不要", "該不該", "能不能", "可不可以", "會不會"];

/// Negations: "不要吃火鍋", "我不吃早餐" are statements, not questions.
#[rustfmt::skip]
const NEGATIONS: &[&str] = &["不要", "不想", "不能", "不該", "不會", "沒有", "不", "沒", "別"];

/// Modal verbs that turn "吃X" into something worth a verdict.
const MODALS: &[&str] = &["可以", "應該", "該", "要", "不如", "乾脆", "還是", "就"];

/// Words that name a meal. Only these anchor a choice without 吃.
#[rustfmt::skip]
const MEAL_WORDS: &[&str] = &[
    "早午餐", "早餐", "早飯", "午餐", "午飯", "中餐", "晚餐", "晚飯", "宵夜", "消夜", "今晚",
];

/// Words for a day or moment that may come before a meal word.
#[rustfmt::skip]
const DAY_WORDS: &[&str] = &[
    "早上", "中午", "晚上", "今天", "明天", "等等", "等一下", "待會兒", "待會", "晚點", "現在",
];

/// Helper verbs that may sit between a time word and 吃: "晚餐要吃".
#[rustfmt::skip]
const HELPER_VERBS: &[&str] = &["應該", "可以", "打算", "要", "就", "來", "該", "想", "會"];

#[rustfmt::skip]
const PRONOUNS: &[&str] = &[
    "你們", "妳們", "我們", "他們", "大家", "你", "妳", "我", "他", "她", "那就", "那", "所以",
];

/// Parses a message. Returns `None` when the message is not a food question.
pub fn parse(content: &str) -> Option<Intent> {
    // Links make the message about something else, and must never be echoed.
    if content.chars().count() > MAX_MESSAGE_CHARS || content.contains("://") {
        return None;
    }

    let message_meal = Meal::detect(content);
    split_clauses(content).iter().find_map(|clause| {
        parse_clause(clause)
            .map(|kind| kind.into_intent(Meal::detect(&clause.text).or(message_meal)))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Clause {
    text: String,
    /// The clause ended with a question mark.
    asked: bool,
}

fn split_clauses(content: &str) -> Vec<Clause> {
    let mut clauses = Vec::new();
    let mut current = String::new();

    let mut push = |current: &mut String, asked: bool| {
        if !current.is_empty() {
            clauses.push(Clause {
                text: std::mem::take(current),
                asked,
            });
        }
    };

    for c in content.chars() {
        if CLAUSE_SEPARATORS.contains(&c) {
            push(&mut current, c == '?' || c == '？');
        } else if !c.is_whitespace() {
            current.push(c);
        }
    }
    push(&mut current, false);
    clauses
}

fn parse_clause(clause: &Clause) -> Option<Kind> {
    let text = clause.text.replace("去吃", "吃").replace("來吃", "吃");
    if PAST_MARKERS.iter().any(|marker| text.contains(marker)) {
        return None;
    }

    text.match_indices('吃')
        .find_map(|(index, eat)| {
            parse_eat(&text[..index], &text[index + eat.len()..], clause.asked)
        })
        .or_else(|| parse_meal_choice(&text))
}

/// Interprets one "吃" given the text before and after it.
fn parse_eat(before: &str, after: &str, asked: bool) -> Option<Kind> {
    // "在" is progressive, except at the end of "現在".
    let describes = ends_with_any(before, NON_QUESTION_BEFORE) && !ends_with_any(before, DAY_WORDS);
    if describes || starts_with_any(after, NON_QUESTION_AFTER) {
        return None;
    }

    // "吃不吃火鍋" or "火鍋吃不吃".
    if let Some(rest) = after.strip_prefix("不吃") {
        if starts_with_any(rest, NON_QUESTION_AFTER) {
            return None;
        }
        let target = if rest.is_empty() {
            strip_subject(before)
        } else {
            rest
        };
        return verdict(target);
    }

    if is_negated(before) {
        return None;
    }

    let cleaned = text::clean_food(after);
    if cleaned.completed {
        return None;
    }

    let question = asked || cleaned.question;
    let wants_verdict = asks_for_verdict(before) || (before.is_empty() && question);
    if cleaned.text.is_empty() {
        // "要不要吃？" with nothing after it: suggest something. But "吃很飽"
        // only looks empty because the comment was cut off.
        return (wants_verdict && !cleaned.cut).then_some(Kind::Recommend);
    }
    match text::generic_kind(&cleaned.text) {
        Some(Generic::Question) => Some(Kind::Recommend),
        // "要不要吃飯" and "吃飯嗎" ask; "我要去吃飯" does not.
        Some(Generic::Meal) => {
            (ends_with_any(before, CHOICE_QUESTIONS) || question).then_some(Kind::Recommend)
        }
        None if wants_verdict => verdict(&cleaned.text),
        None => None,
    }
}

/// "晚餐火鍋還是拉麵": a choice question without 吃. It needs a meal word up
/// front and at least one option that looks like a dish, so "今天下雨還是晴天"
/// stays quiet.
fn parse_meal_choice(text: &str) -> Option<Kind> {
    if !text::has_option_separator(text) {
        return None;
    }

    let rest = strip_prefix_any(text, PRONOUNS);
    let rest = strip_prefix_any(rest, DAY_WORDS);
    let rest = MEAL_WORDS.iter().find_map(|word| rest.strip_prefix(word))?;
    let rest = strip_prefix_any(rest, HELPER_VERBS);
    match verdict(&text::clean_food(rest).text)? {
        Kind::Choose(options) if options.iter().any(|option| text::looks_like_food(option)) => {
            Some(Kind::Choose(options))
        }
        _ => None,
    }
}

/// Builds a verdict for one food, or a choice for several.
fn verdict(target: &str) -> Option<Kind> {
    let mut options = text::parse_options(target).ok()?;
    match options.len() {
        0 => None,
        1 => options.pop().map(Kind::Judge),
        count if count <= MAX_OPTIONS => Some(Kind::Choose(options)),
        _ => None,
    }
}

fn is_negated(before: &str) -> bool {
    !ends_with_any(before, CHOICE_QUESTIONS) && ends_with_any(before, NEGATIONS)
}

/// True when the words before 吃 make this a question about one food.
fn asks_for_verdict(before: &str) -> bool {
    if ends_with_any(before, CHOICE_QUESTIONS) {
        return true;
    }
    if is_negated(before) {
        return false;
    }
    if ends_with_any(before, MODALS) {
        return true;
    }
    let trimmed = HELPER_VERBS
        .iter()
        .find_map(|verb| before.strip_suffix(verb))
        .unwrap_or(before);
    ends_with_any(trimmed, MEAL_WORDS) || ends_with_any(trimmed, DAY_WORDS)
}

/// Removes leading pronouns, time words and helper verbs: "你今天火鍋" -> "火鍋".
fn strip_subject(text: &str) -> &str {
    let mut rest = text;
    loop {
        let next = strip_prefix_any(rest, PRONOUNS);
        let next = strip_prefix_any(next, DAY_WORDS);
        let next = strip_prefix_any(next, MEAL_WORDS);
        let next = strip_prefix_any(next, HELPER_VERBS);
        if next.len() == rest.len() {
            return rest;
        }
        rest = next;
    }
}

fn strip_prefix_any<'a>(text: &'a str, prefixes: &[&str]) -> &'a str {
    prefixes
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .unwrap_or(text)
}

fn ends_with_any(text: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| text.ends_with(suffix))
}

fn starts_with_any(text: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| text.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judge(food: &str, meal: Option<Meal>) -> Option<Intent> {
        Some(Intent::Judge {
            food: food.to_owned(),
            meal,
        })
    }

    fn choose(options: &[&str], meal: Option<Meal>) -> Option<Intent> {
        Some(Intent::Choose {
            options: options.iter().map(|option| (*option).to_owned()).collect(),
            meal,
        })
    }

    fn recommend(meal: Option<Meal>) -> Option<Intent> {
        Some(Intent::Recommend { meal })
    }

    #[test]
    fn open_questions_ask_for_a_recommendation() {
        assert_eq!(parse("晚餐吃什麼？"), recommend(Some(Meal::Dinner)));
        assert_eq!(parse("吃什麼"), recommend(None));
        assert_eq!(parse("今天吃啥"), recommend(None));
        assert_eq!(parse("宵夜要吃啥好呢"), recommend(Some(Meal::LateNight)));
        assert_eq!(parse("中午吃什麼比較好"), recommend(Some(Meal::Lunch)));
        assert_eq!(parse("大家晚餐吃什麼呢"), recommend(Some(Meal::Dinner)));
        assert_eq!(parse("要不要吃點東西"), recommend(None));
    }

    #[test]
    fn keeps_the_original_bots_test_cases() {
        assert_eq!(
            parse("好吧說實話我現在其實沒有食慾只是我覺得應該吃些什麼"),
            recommend(None)
        );
        assert_eq!(parse("我覺得應該吃些什麼"), recommend(None));
        assert_eq!(parse("要不要吃火鍋？"), judge("火鍋", None));
        assert_eq!(parse("晚餐吃 拉麵 啊？"), judge("拉麵", Some(Meal::Dinner)));
    }

    #[test]
    fn named_foods_get_a_verdict() {
        assert_eq!(parse("要不要吃個火鍋"), judge("火鍋", None));
        assert_eq!(parse("要不要去吃麥當勞"), judge("麥當勞", None));
        assert_eq!(
            parse("我晚餐吃麥當勞好了"),
            judge("麥當勞", Some(Meal::Dinner))
        );
        assert_eq!(parse("今天吃火鍋嗎"), judge("火鍋", None));
        assert_eq!(parse("吃不吃火鍋"), judge("火鍋", None));
        assert_eq!(parse("你火鍋吃不吃"), judge("火鍋", None));
        assert_eq!(parse("可以吃炸雞嗎"), judge("炸雞", None));
        assert_eq!(parse("吃拉麵？"), judge("拉麵", None));
        assert_eq!(parse("晚餐想吃牛肉麵"), judge("牛肉麵", Some(Meal::Dinner)));
    }

    #[test]
    fn choice_questions_offer_options() {
        assert_eq!(
            parse("要不要吃火鍋還是拉麵"),
            choose(&["火鍋", "拉麵"], None)
        );
        assert_eq!(
            parse("宵夜吃鹽酥雞還是滷味或臭豆腐"),
            choose(&["鹽酥雞", "滷味", "臭豆腐"], Some(Meal::LateNight))
        );
        assert_eq!(
            parse("晚餐火鍋還是拉麵？"),
            choose(&["火鍋", "拉麵"], Some(Meal::Dinner))
        );
        assert_eq!(parse("要不要吃火鍋還是火鍋"), judge("火鍋", None));
    }

    #[test]
    fn trailing_chatter_is_not_part_of_the_food() {
        assert_eq!(parse("我不知道該吃什麼但我想要大便"), recommend(None));
        assert_eq!(parse("要不要吃火鍋，好冷喔"), judge("火鍋", None));
    }

    #[test]
    fn past_and_progressive_questions_are_ignored() {
        assert_eq!(parse("你晚餐吃了沒"), None);
        assert_eq!(parse("今天吃飯了嗎"), None);
        assert_eq!(parse("吃過了"), None);
        assert_eq!(parse("昨天晚餐吃什麼"), None);
        assert_eq!(parse("你在吃什麼藥"), None);
        assert_eq!(parse("你在吃什麼"), None);
    }

    #[test]
    fn later_clauses_still_count() {
        assert_eq!(parse("昨天吃火鍋，今天吃什麼"), recommend(None));
    }

    #[test]
    fn non_food_is_ignored() {
        assert_eq!(parse("該吃藥了"), None);
        assert_eq!(parse("要不要吃藥"), None);
        assert_eq!(parse("你要不要吃屎"), None);
        assert_eq!(parse("我又在吃醋"), None);
        assert_eq!(parse("月底要吃土"), None);
    }

    #[test]
    fn mentions_and_markup_are_never_echoed() {
        assert_eq!(parse("要不要吃@everyone"), None);
        assert_eq!(parse("要不要吃<@123456789>"), None);
        assert_eq!(parse("要不要吃火鍋還是@here"), None);
        assert_eq!(parse("要不要吃**火鍋**"), None);
        assert_eq!(parse("要不要吃https://example.com"), None);
    }

    #[test]
    fn statements_and_small_talk_are_ignored() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("今天天氣真好"), None);
        assert_eq!(parse("我想吃火鍋"), None);
        assert_eq!(parse("不要吃火鍋"), None);
        assert_eq!(parse("這家好吃嗎"), None);
        assert_eq!(parse("夜市小吃好多"), None);
        assert_eq!(parse("吃不下了"), None);
        assert_eq!(parse("吃什麼都好"), None);
        assert_eq!(parse("火鍋還是拉麵"), None);
    }

    /// Inputs from the code review that used to trigger wrongly.
    #[test]
    fn review_regressions_stay_quiet() {
        for message in [
            // Meal words in statements and negations.
            "我先去吃飯",
            "大家吃飯囉",
            "回家吃飯",
            "吃飯吧",
            "我不想吃飯",
            "我不吃早餐",
            "減肥不吃晚餐",
            "不要吃東西",
            "我不想吃什麼",
            // Choices without 吃 that are not about food.
            "今天下雨還是晴天",
            "晚上看電影還是打球",
            "今天湖人vs勇士",
            "你們今天要去還是不去",
            "晚餐看電影還是打球",
            // Comments and adverbs mistaken for food.
            "今天吃很飽",
            "你到底吃不吃",
            "你還要吃不吃",
            "你先吃不吃",
            "吃不吃得下",
            "要吃這個嗎",
            "要吃多少",
            "要吃幾個",
            "要不要吃一頓好的",
            // Invisible filler as the whole food.
            "要不要吃\u{3164}",
            // Second review pass.
            "我要去吃飯",
            "每天都要吃早餐",
            "你平常都吃什麼",
            "應該吃多一點",
            "要不要吃屁",
            "要不要吃a.b.c",
        ] {
            assert_eq!(parse(message), None, "{message}");
        }
    }

    #[test]
    fn review_regressions_answer_correctly() {
        assert_eq!(parse("晚餐吃牛排很開心"), judge("牛排", Some(Meal::Dinner)));
        assert_eq!(
            parse("今天中午吃便當很好吃"),
            judge("便當", Some(Meal::Lunch))
        );
        assert_eq!(
            parse("要吃火鍋還是要吃拉麵"),
            choose(&["火鍋", "拉麵"], None)
        );
        assert_eq!(parse("要不要吃火鍋還是不要"), judge("火鍋", None));
        assert_eq!(parse("現在吃什麼"), recommend(None));
        assert_eq!(parse("那現在吃什麼"), recommend(None));
        assert_eq!(parse("現在吃火鍋好嗎"), judge("火鍋", None));
        assert_eq!(parse("要不要吃下午茶"), judge("下午茶", None));
        assert_eq!(parse("要不要吃到飽"), judge("吃到飽", None));
        assert_eq!(parse("要不要吃飯"), recommend(None));
        assert_eq!(parse("今天要吃飯嗎"), recommend(None));
        assert_eq!(parse("晚餐吃火鍋好開心"), judge("火鍋", Some(Meal::Dinner)));
        assert_eq!(parse("吃飯嗎？"), recommend(None));
        assert_eq!(
            parse("今天晚餐火鍋還是拉麵"),
            choose(&["火鍋", "拉麵"], Some(Meal::Dinner))
        );
        assert_eq!(
            parse("今晚火鍋還是燒烤"),
            choose(&["火鍋", "燒烤"], Some(Meal::Dinner))
        );
    }

    #[test]
    fn long_messages_are_ignored() {
        let long = format!("{}晚餐吃什麼", "啊".repeat(MAX_MESSAGE_CHARS));
        assert_eq!(parse(&long), None);
        let limit = format!("{}晚餐吃什麼", "啊".repeat(MAX_MESSAGE_CHARS - 5));
        assert_eq!(parse(&limit), recommend(Some(Meal::Dinner)));
    }

    #[test]
    fn too_many_options_are_ignored() {
        let options: Vec<String> = (0..=MAX_OPTIONS).map(|n| format!("菜{n}")).collect();
        assert_eq!(parse(&format!("要不要吃{}", options.join("還是"))), None);
    }

    #[test]
    fn long_food_names_are_ignored() {
        assert_eq!(
            parse(&format!(
                "要不要吃{}",
                "肉".repeat(text::MAX_FOOD_CHARS + 1)
            )),
            None
        );
    }

    #[test]
    fn split_clauses_tracks_question_marks() {
        assert_eq!(
            split_clauses("吃拉麵？好 啊"),
            vec![
                Clause {
                    text: "吃拉麵".to_owned(),
                    asked: true
                },
                Clause {
                    text: "好啊".to_owned(),
                    asked: false
                },
            ]
        );
        assert!(split_clauses("？？ ，").is_empty());
    }
}
