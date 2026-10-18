//! Cleaning and validating food names that users type.
//!
//! Anything that ends up inside a bot reply passes through [`validate_food`],
//! which only accepts letters, digits and a few harmless symbols. That rules
//! out mentions (`@everyone`, `<@id>`), links, emoji and Discord markdown.

/// Longest food name accepted from chat, in characters.
pub const MAX_FOOD_CHARS: usize = 16;

/// Most options a single "A 還是 B" question may offer.
pub const MAX_OPTIONS: usize = 10;

/// Symbols allowed in a food name besides letters and digits.
const ALLOWED_SYMBOLS: &[char] = &['-', '&', '\'', '·', '‧', '.', '+'];

/// Words that end the food phrase: whatever follows is a new thought.
const CUT_WORDS: &[&str] = &[
    "但是",
    "但",
    "可是",
    "不過",
    "因為",
    "所以",
    "然後",
    "結果",
    "不然",
    "我",
    "你覺得",
    "你說",
    "大家",
];

/// Fillers that may precede the food: "要不要吃個火鍋".
const LEADING_FILLERS: &[&str] = &[
    "一個", "一碗", "一份", "一頓", "一點", "一些", "一下", "個", "些", "吃",
];

/// Particles and tags that may follow the food, longest first.
const TRAILING: &[&str] = &[
    "怎麼樣",
    "可不可以",
    "好不好",
    "行不行",
    "比較好",
    "可以嗎",
    "看看",
    "好嗎",
    "好了",
    "如何",
    "怎樣",
    "行嗎",
    "才好",
    "一下",
    "好呢",
    "呢",
    "嗎",
    "吗",
    "啊",
    "阿",
    "啦",
    "吧",
    "呀",
    "喔",
    "哦",
    "耶",
    "欸",
    "誒",
    "勒",
    "咧",
    "捏",
    "嘛",
    "喇",
    "齁",
    "囉",
    "了",
    "沒",
    "没",
    "過",
    "好",
];

/// Trailing markers meaning the eating already happened: "吃了沒".
const COMPLETED_MARKERS: &[&str] = &["了", "沒", "没", "過"];

/// Trailing markers that turn the phrase into a question.
const QUESTION_MARKERS: &[&str] = &[
    "嗎",
    "吗",
    "好嗎",
    "好不好",
    "如何",
    "怎樣",
    "怎麼樣",
    "可以嗎",
    "行嗎",
    "行不行",
    "可不可以",
    "呢",
];

/// Separators between options in "火鍋還是拉麵".
const OPTION_SEPARATORS: &[&str] = &[
    "還是", "或是", "或者", "或", "、", "/", "／", "vs", "VS", "Vs", ",", "，",
];

/// Phrases that ask for a suggestion instead of naming a food.
const GENERIC_WORDS: &[&str] = &[
    "什麼",
    "甚麼",
    "什么",
    "啥",
    "啥子",
    "蝦米",
    "哪個",
    "哪一個",
    "哪家",
    "哪間",
    "哪種",
    "哪一種",
    "飯",
    "東西",
    "早餐",
    "午餐",
    "晚餐",
    "宵夜",
    "消夜",
    "正餐",
    "大餐",
];

/// Question words that make a phrase a question rather than a food.
const QUESTION_WORDS: &[&str] = &["什麼", "甚麼", "什么", "啥", "哪"];

/// Things people "eat" in Chinese idioms or jokes that are not food.
const NOT_FOOD: &[&str] = &[
    "藥",
    "屎",
    "大便",
    "土",
    "虧",
    "醋",
    "苦",
    "苦頭",
    "軟飯",
    "閉門羹",
    "鱉",
    "癟",
    "香",
    "驚",
    "力",
    "霸王餐",
    "紅牌",
    "黃牌",
    "子彈",
    "螺絲",
    "官司",
    "罰單",
    "老本",
    "草",
    "空氣",
    "西北風",
    "人",
    "我",
    "你",
    "妳",
    "他",
    "她",
    "自己",
];

/// Suffixes that mark medicine or worse.
const NOT_FOOD_SUFFIXES: &[&str] = &["藥", "藥丸", "藥水", "屎", "大便"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoodRejection {
    Empty,
    TooLong,
    UnsafeCharacter,
    /// A question word such as 什麼: the user wants a suggestion.
    Generic,
    NotFood,
}

/// A food phrase with surrounding chatter removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    pub text: String,
    /// A completed-aspect marker such as 了 or 沒 was removed.
    pub completed: bool,
    /// A question marker such as 嗎 was removed.
    pub question: bool,
}

/// Removes whitespace, trailing clauses, fillers and particles around a food.
pub fn clean_food(raw: &str) -> Cleaned {
    let mut text: String = raw.chars().filter(|c| !c.is_whitespace()).collect();

    if let Some(cut) = CUT_WORDS.iter().filter_map(|word| text.find(word)).min() {
        text.truncate(cut);
    }

    let mut start = 0;
    while let Some(filler) = LEADING_FILLERS
        .iter()
        .find(|filler| text[start..].starts_with(*filler))
    {
        start += filler.len();
    }
    text.drain(..start);

    let mut completed = false;
    let mut question = false;
    while let Some(tail) = TRAILING.iter().find(|tail| text.ends_with(*tail)) {
        completed |= COMPLETED_MARKERS.contains(tail);
        question |= QUESTION_MARKERS.contains(tail);
        text.truncate(text.len() - tail.len());
    }

    Cleaned {
        text,
        completed,
        question,
    }
}

/// True when the phrase asks for a suggestion: 什麼, 點什麼, 啥東西, 飯...
pub fn is_generic(food: &str) -> bool {
    let mut core = food;
    for prefix in ["一點", "點", "一些", "些"] {
        core = core.strip_prefix(prefix).unwrap_or(core);
    }
    for suffix in ["東西", "比較好", "好"] {
        core = core.strip_suffix(suffix).unwrap_or(core);
    }
    core.is_empty() || GENERIC_WORDS.contains(&core)
}

/// Checks that a cleaned food name is safe and sensible to echo back.
pub fn validate_food(food: &str) -> Result<(), FoodRejection> {
    if food.is_empty() {
        return Err(FoodRejection::Empty);
    }
    if is_generic(food) {
        return Err(FoodRejection::Generic);
    }
    if food.chars().count() > MAX_FOOD_CHARS {
        return Err(FoodRejection::TooLong);
    }
    if !food
        .chars()
        .all(|c| c.is_alphanumeric() || ALLOWED_SYMBOLS.contains(&c))
    {
        return Err(FoodRejection::UnsafeCharacter);
    }
    if QUESTION_WORDS.iter().any(|word| food.contains(word))
        || NOT_FOOD.contains(&food)
        || NOT_FOOD_SUFFIXES
            .iter()
            .any(|suffix| food.ends_with(suffix))
    {
        return Err(FoodRejection::NotFood);
    }
    Ok(())
}

pub fn has_option_separator(text: &str) -> bool {
    OPTION_SEPARATORS
        .iter()
        .any(|separator| text.contains(separator))
}

/// Splits "火鍋還是拉麵" (or "火鍋 拉麵") into cleaned, validated, unique options.
///
/// Generic parts such as "還是什麼" are dropped. Any part that is unsafe, too
/// long or not food rejects the whole list, so nothing odd gets echoed.
pub fn parse_options(text: &str) -> Result<Vec<String>, FoodRejection> {
    let mut normalized = text.to_owned();
    for separator in OPTION_SEPARATORS {
        normalized = normalized.replace(separator, "\n");
    }

    let mut options: Vec<String> = Vec::new();
    for part in normalized.split(char::is_whitespace) {
        let cleaned = clean_food(part).text;
        match validate_food(&cleaned) {
            Ok(()) => {
                if !options.contains(&cleaned) {
                    options.push(cleaned);
                }
            }
            Err(FoodRejection::Empty | FoodRejection::Generic) => {}
            Err(rejection) => return Err(rejection),
        }
    }
    Ok(options)
}

/// Checks text from the data file before it is used in replies: no markup,
/// mentions or control characters, and a reasonable length.
pub fn is_safe_display_text(text: &str, max_chars: usize) -> bool {
    const FORBIDDEN: &[char] = &['@', '<', '>', '`', '*', '_', '~', '|', '\\', '[', ']', '#'];
    let length = text.chars().count();
    length > 0
        && length <= max_chars
        && !text.contains("://")
        && text
            .chars()
            .all(|c| !c.is_control() && !FORBIDDEN.contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleaned(raw: &str) -> String {
        clean_food(raw).text
    }

    #[test]
    fn clean_strips_particles_and_whitespace() {
        assert_eq!(cleaned("火鍋嗎"), "火鍋");
        assert_eq!(cleaned(" 拉麵 啊"), "拉麵");
        assert_eq!(cleaned("麥當勞好了"), "麥當勞");
        assert_eq!(cleaned("火鍋好不好啦"), "火鍋");
    }

    #[test]
    fn clean_strips_leading_fillers() {
        assert_eq!(cleaned("個火鍋"), "火鍋");
        assert_eq!(cleaned("一碗牛肉麵"), "牛肉麵");
    }

    #[test]
    fn clean_cuts_at_new_clauses() {
        assert_eq!(cleaned("什麼但我想要大便"), "什麼");
        assert_eq!(cleaned("火鍋因為好冷"), "火鍋");
    }

    #[test]
    fn clean_flags_completed_and_question_markers() {
        let completed = clean_food("了沒");
        assert!(completed.completed);
        assert_eq!(completed.text, "");

        let question = clean_food("火鍋嗎");
        assert!(question.question);
        assert!(!question.completed);

        // 好了 means "decided", not "already eaten".
        assert!(!clean_food("麥當勞好了").completed);
    }

    #[test]
    fn clean_handles_empty_input() {
        assert_eq!(
            clean_food(""),
            Cleaned {
                text: String::new(),
                completed: false,
                question: false
            }
        );
        assert_eq!(cleaned("   "), "");
    }

    #[test]
    fn generic_phrases_are_detected() {
        for phrase in [
            "什麼",
            "啥",
            "點什麼",
            "一點什麼東西",
            "些什麼",
            "什麼好",
            "飯",
            "東西",
            "",
        ] {
            assert!(is_generic(phrase), "{phrase} should be generic");
        }
        assert!(!is_generic("火鍋"));
        assert!(!is_generic("什麼藥"));
    }

    #[test]
    fn validate_accepts_ordinary_food() {
        for food in [
            "火鍋",
            "牛肉麵",
            "OREO冰炫風",
            "Costco熱狗",
            "85度C",
            "炸和牛棒球堡",
        ] {
            assert_eq!(validate_food(food), Ok(()), "{food}");
        }
    }

    #[test]
    fn validate_rejects_mentions_links_and_markup() {
        for text in [
            "@everyone",
            "<@123456789>",
            "<#1>",
            "http://x.y",
            "**粗體**",
            "火鍋🍲",
            "a|b",
            "`code`",
        ] {
            assert_eq!(
                validate_food(text),
                Err(FoodRejection::UnsafeCharacter),
                "{text}"
            );
        }
    }

    #[test]
    fn validate_rejects_non_food_and_question_words() {
        for text in ["藥", "感冒藥", "屎", "醋", "土", "什麼藥", "哪裡"] {
            assert_eq!(validate_food(text), Err(FoodRejection::NotFood), "{text}");
        }
    }

    #[test]
    fn validate_checks_length_boundaries() {
        let longest = "肉".repeat(MAX_FOOD_CHARS);
        assert_eq!(validate_food(&longest), Ok(()));
        let too_long = "肉".repeat(MAX_FOOD_CHARS + 1);
        assert_eq!(validate_food(&too_long), Err(FoodRejection::TooLong));
        assert_eq!(validate_food(""), Err(FoodRejection::Empty));
        assert_eq!(validate_food("什麼"), Err(FoodRejection::Generic));
    }

    #[test]
    fn parse_options_splits_and_dedupes() {
        assert_eq!(
            parse_options("火鍋還是拉麵"),
            Ok(vec!["火鍋".to_owned(), "拉麵".to_owned()])
        );
        assert_eq!(
            parse_options("火鍋 拉麵、壽司/火鍋"),
            Ok(vec![
                "火鍋".to_owned(),
                "拉麵".to_owned(),
                "壽司".to_owned()
            ])
        );
        assert_eq!(
            parse_options("火鍋還是吃拉麵呢"),
            Ok(vec!["火鍋".to_owned(), "拉麵".to_owned()])
        );
    }

    #[test]
    fn parse_options_drops_generic_parts() {
        assert_eq!(parse_options("火鍋還是什麼"), Ok(vec!["火鍋".to_owned()]));
        assert_eq!(parse_options(""), Ok(vec![]));
    }

    #[test]
    fn parse_options_rejects_unsafe_parts() {
        assert_eq!(
            parse_options("火鍋還是@everyone"),
            Err(FoodRejection::UnsafeCharacter)
        );
        assert_eq!(parse_options("火鍋還是藥"), Err(FoodRejection::NotFood));
    }

    #[test]
    fn safe_display_text_blocks_markup() {
        assert!(is_safe_display_text("冰箱裡那盒不知道是什麼的東西", 32));
        assert!(!is_safe_display_text("", 32));
        assert!(!is_safe_display_text("@here", 32));
        assert!(!is_safe_display_text("https://example.com", 32));
        assert!(!is_safe_display_text("line\nbreak", 32));
        assert!(!is_safe_display_text("太長了", 2));
    }
}
