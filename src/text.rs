//! Cleaning and validating food names that users type.
//!
//! Anything that ends up inside a bot reply passes through [`validate_food`],
//! which only accepts characters from a few scripts plus harmless symbols.
//! That rules out mentions (`@everyone`, `<@id>`), links, emoji, Discord
//! markdown and invisible or combining characters.

/// Longest food name accepted from chat, in characters.
pub const MAX_FOOD_CHARS: usize = 16;

/// Most options a single "A 還是 B" question may offer.
pub const MAX_OPTIONS: usize = 10;

/// Symbols allowed in a food name besides letters and digits.
const ALLOWED_SYMBOLS: &[char] = &['-', '&', '\'', '·', '‧', '.', '+'];

/// Words that end the food phrase: whatever follows is a new thought or a
/// comment about the food ("牛排很好吃").
#[rustfmt::skip]
const CUT_WORDS: &[&str] = &[
    "但是", "但", "可是", "不過", "因為", "所以", "然後", "結果", "不然", "我", "你覺得", "你說", "大家",
    "很", "好吃", "真的", "有夠",
];

/// Fillers that may precede the food: "要不要吃個火鍋", "還是要吃拉麵".
#[rustfmt::skip]
const LEADING_FILLERS: &[&str] = &[
    "一個", "一碗", "一份", "一頓", "一點", "一些", "一下", "要吃", "想吃", "去吃", "來吃", "個", "些", "吃",
    "要", "想",
];

/// Particles and tags that may follow the food, longest first.
#[rustfmt::skip]
const TRAILING: &[&str] = &[
    "怎麼樣", "可不可以", "好不好", "行不行", "比較好", "可以嗎", "看看", "好嗎", "好了", "如何", "怎樣", "行嗎",
    "才好", "一下", "好呢", "呢", "嗎", "吗", "啊", "阿", "啦", "吧", "呀", "喔", "哦", "耶", "欸", "誒", "勒",
    "咧", "捏", "嘛", "喇", "齁", "囉", "了", "沒", "没", "過", "好",
];

/// Trailing markers meaning the eating already happened: "吃了沒".
const COMPLETED_MARKERS: &[&str] = &["了", "沒", "没", "過"];

/// Trailing markers that turn the phrase into a question.
#[rustfmt::skip]
const QUESTION_MARKERS: &[&str] = &[
    "嗎", "吗", "好嗎", "好不好", "如何", "怎樣", "怎麼樣", "可以嗎", "行嗎", "行不行", "可不可以", "呢",
];

/// Separators between options in "火鍋還是拉麵".
#[rustfmt::skip]
const OPTION_SEPARATORS: &[&str] = &[
    "還是", "或是", "或者", "或", "、", "/", "／", "vs", "VS", "Vs", ",", "，",
];

/// Question words asking for a suggestion: "吃什麼".
#[rustfmt::skip]
const QUESTION_GENERICS: &[&str] = &[
    "什麼", "甚麼", "什么", "啥", "啥子", "蝦米", "哪個", "哪一個", "哪家", "哪間", "哪種", "哪一種",
];

/// Words for "a meal" in general: "吃飯". They only ask for a suggestion
/// when the sentence is a question.
#[rustfmt::skip]
const MEAL_GENERICS: &[&str] = &[
    "飯", "東西", "早餐", "早飯", "午餐", "午飯", "晚餐", "晚飯", "宵夜", "消夜", "正餐", "大餐",
];

/// Question words that make a phrase a question rather than a food.
const QUESTION_WORDS: &[&str] = &["什麼", "甚麼", "什么", "啥", "哪"];

/// Answers that decline instead of naming a food: "火鍋還是不要".
#[rustfmt::skip]
const DISMISSIVE: &[&str] = &[
    "不要", "不吃", "都不要", "算了", "都可以", "都好", "都行", "隨便",
];

/// Things people "eat" in Chinese idioms or jokes that are not food.
#[rustfmt::skip]
const NOT_FOOD: &[&str] = &[
    "藥", "屎", "大便", "土", "虧", "醋", "苦", "苦頭", "軟飯", "閉門羹", "鱉", "癟", "香", "驚", "力", "霸王餐",
    "紅牌", "黃牌", "子彈", "螺絲", "官司", "罰單", "老本", "草", "空氣", "西北風", "人", "我", "你", "妳", "他",
    "她", "自己", "好的", "屁",
];

/// Suffixes that mark medicine or worse.
const NOT_FOOD_SUFFIXES: &[&str] = &["藥", "藥丸", "藥水", "屎", "大便", "屁"];

/// Adverbs, demonstratives and quantities that cannot start a food name:
/// "吃很飽", "吃這個", "吃多少", "到底吃不吃".
#[rustfmt::skip]
const NOT_FOOD_PREFIXES: &[&str] = &[
    "很", "太多", "太少", "太飽", "超飽", "好飽", "好多", "真的", "有夠", "蠻", "挺", "不", "沒", "得", "多少",
    "幾", "這", "那", "到底", "還", "先", "再", "也", "都", "又", "一樣", "完", "多一", "多點", "少",
];

/// Characters that start a comment when they appear after the food:
/// "火鍋好開心", "拉麵超讚".
const COMMENT_CHARS: &[char] = &['好', '超', '真', '很'];

/// Characters and words that make a phrase look like a dish.
#[rustfmt::skip]
const FOOD_HINTS: &[&str] = &[
    "飯", "麵", "面", "鍋", "雞", "鴨", "豬", "牛", "羊", "魚", "蝦", "肉", "蛋", "堡", "湯", "餃", "粥", "包",
    "餅", "排", "壽司", "披薩", "炸", "烤", "滷", "丼", "粉", "粿", "糕", "捲", "卷", "串", "燒", "沙拉", "三明治",
    "便當", "咖哩", "豆腐", "餛飩", "吐司", "薯條", "麥當勞", "肯德基", "摩斯", "鹽酥雞", "關東煮", "熱炒",
    "點心", "甜點", "早午餐", "冰", "茶", "咖啡", "粽", "飯糰",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoodRejection {
    Empty,
    TooLong,
    UnsafeCharacter,
    /// A question word or "a meal": the user wants a suggestion.
    Generic,
    NotFood,
}

/// How a phrase asks for a suggestion instead of naming a food.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generic {
    /// 什麼, 啥, 哪家: always a question.
    Question,
    /// 飯, 東西, 晚餐: a question only when the sentence is one.
    Meal,
}

/// A food phrase with surrounding chatter removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    pub text: String,
    /// A completed-aspect marker such as 了 or 沒 was removed.
    pub completed: bool,
    /// A question marker such as 嗎 was removed.
    pub question: bool,
    /// Something after the food was cut off ("很飽", "但...").
    pub cut: bool,
}

/// Removes whitespace, trailing clauses, fillers and particles around a food.
pub fn clean_food(raw: &str) -> Cleaned {
    let mut text: String = raw.chars().filter(|c| !c.is_whitespace()).collect();

    let cut_at = CUT_WORDS.iter().filter_map(|word| text.find(word)).min();
    if let Some(cut) = cut_at {
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

    // "吃到飽" is a buffet, and the 吃 was consumed by the parser.
    if text.starts_with("到飽") {
        text.insert(0, '吃');
    }

    let mut completed = false;
    let mut question = false;
    let mut strip_trailing = |text: &mut String| {
        while let Some(tail) = TRAILING.iter().find(|tail| text.ends_with(*tail)) {
            completed |= COMPLETED_MARKERS.contains(tail);
            question |= QUESTION_MARKERS.contains(tail);
            text.truncate(text.len() - tail.len());
        }
    };
    strip_trailing(&mut text);

    // After the particles are gone, a 好 or 超 past the first character
    // starts a comment: "火鍋好開心". Leading ones belong to the name.
    let comment_at = text
        .char_indices()
        .skip(1)
        .find(|(_, c)| COMMENT_CHARS.contains(c))
        .map(|(index, _)| index);
    if let Some(index) = comment_at {
        text.truncate(index);
        strip_trailing(&mut text);
    }

    Cleaned {
        text,
        completed,
        question,
        cut: cut_at.is_some() || comment_at.is_some(),
    }
}

/// Tells whether the phrase asks for a suggestion: 什麼, 點什麼, 啥東西, 飯...
pub fn generic_kind(food: &str) -> Option<Generic> {
    let mut core = food;
    for prefix in ["一點", "點", "一些", "些"] {
        core = core.strip_prefix(prefix).unwrap_or(core);
    }
    for suffix in ["東西", "比較好", "好"] {
        core = core.strip_suffix(suffix).unwrap_or(core);
    }

    if QUESTION_GENERICS.contains(&core) {
        Some(Generic::Question)
    } else if core.is_empty() || MEAL_GENERICS.contains(&core) {
        Some(Generic::Meal)
    } else {
        None
    }
}

pub fn is_generic(food: &str) -> bool {
    generic_kind(food).is_some()
}

/// Letters and digits from the scripts food names are written in. Anything
/// else (emoji, symbols, invisible fillers, combining marks, right-to-left
/// text) is refused.
fn is_food_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || ALLOWED_SYMBOLS.contains(&c)
        || matches!(c,
            '\u{4E00}'..='\u{9FFF}'   // CJK unified ideographs
            | '\u{3400}'..='\u{4DBF}' // CJK extension A
            | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
            | '\u{3041}'..='\u{3096}' // hiragana
            | '\u{30A1}'..='\u{30FA}' // katakana
            | '\u{30FC}'              // katakana long vowel mark
            | '\u{AC00}'..='\u{D7A3}' // hangul syllables
            | '\u{FF10}'..='\u{FF19}' // fullwidth digits
            | '\u{FF21}'..='\u{FF3A}' // fullwidth A-Z
            | '\u{FF41}'..='\u{FF5A}' // fullwidth a-z
            | '\u{00C0}'..='\u{00D6}' // Latin-1 letters such as é
            | '\u{00D8}'..='\u{00F6}'
            | '\u{00F8}'..='\u{00FF}')
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
    if !food.chars().all(is_food_char) {
        return Err(FoodRejection::UnsafeCharacter);
    }
    // A lone Latin letter or digit is a typo or a fragment, not a dish.
    if food.chars().count() < 2 && food.is_ascii() {
        return Err(FoodRejection::NotFood);
    }
    if QUESTION_WORDS.iter().any(|word| food.contains(word))
        || NOT_FOOD.contains(&food)
        || NOT_FOOD_SUFFIXES
            .iter()
            .any(|suffix| food.ends_with(suffix))
        || NOT_FOOD_PREFIXES
            .iter()
            .any(|prefix| food.starts_with(prefix))
    {
        return Err(FoodRejection::NotFood);
    }
    Ok(())
}

/// A rough check that a phrase names a dish, used where there is no "吃"
/// to go on ("晚餐火鍋還是拉麵").
pub fn looks_like_food(text: &str) -> bool {
    FOOD_HINTS.iter().any(|hint| text.contains(hint))
}

pub fn has_option_separator(text: &str) -> bool {
    OPTION_SEPARATORS
        .iter()
        .any(|separator| text.contains(separator))
}

/// Splits "火鍋還是拉麵" (or "火鍋 拉麵") into cleaned, validated, unique options.
///
/// Generic and declining parts such as "還是什麼" or "還是不要" are dropped.
/// Any part that is unsafe, too long or not food rejects the whole list, so
/// nothing odd gets echoed.
pub fn parse_options(text: &str) -> Result<Vec<String>, FoodRejection> {
    let mut normalized = text.to_owned();
    for separator in OPTION_SEPARATORS {
        normalized = normalized.replace(separator, "\n");
    }

    let mut options: Vec<String> = Vec::new();
    for part in normalized.split(char::is_whitespace) {
        let cleaned = clean_food(part).text;
        if DISMISSIVE.contains(&cleaned.as_str()) {
            continue;
        }
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

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
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
        assert_eq!(cleaned("要吃拉麵"), "拉麵");
    }

    #[test]
    fn clean_cuts_at_new_clauses_and_comments() {
        assert_eq!(cleaned("什麼但我想要大便"), "什麼");
        assert_eq!(cleaned("火鍋因為好冷"), "火鍋");
        assert_eq!(cleaned("牛排很開心"), "牛排");
        assert_eq!(cleaned("便當很好吃"), "便當");
        assert!(clean_food("很飽").cut);
        assert!(!clean_food("火鍋").cut);
    }

    #[test]
    fn clean_cuts_comments_after_the_food() {
        assert_eq!(cleaned("火鍋好開心"), "火鍋");
        assert_eq!(cleaned("拉麵超讚"), "拉麵");
        assert_eq!(cleaned("火鍋好好吃喔"), "火鍋");
        assert!(clean_food("火鍋好開心").cut);
        // Leading 好 is part of the name, and 好不好 is a question.
        assert_eq!(cleaned("好多蛋三明治"), "好多蛋三明治");
        let question = clean_food("火鍋好不好");
        assert_eq!(question.text, "火鍋");
        assert!(question.question);
        assert!(!question.cut);
    }

    #[test]
    fn clean_restores_all_you_can_eat() {
        assert_eq!(cleaned("到飽"), "吃到飽");
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
                question: false,
                cut: false
            }
        );
        assert_eq!(cleaned("   "), "");
    }

    #[test]
    fn generic_kinds_are_told_apart() {
        for phrase in [
            "什麼",
            "啥",
            "點什麼",
            "一點什麼東西",
            "些什麼",
            "什麼好",
            "哪家",
        ] {
            assert_eq!(generic_kind(phrase), Some(Generic::Question), "{phrase}");
        }
        for phrase in ["飯", "東西", "點東西", "晚餐", ""] {
            assert_eq!(generic_kind(phrase), Some(Generic::Meal), "{phrase}");
        }
        assert_eq!(generic_kind("火鍋"), None);
        assert_eq!(generic_kind("什麼藥"), None);
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
            "ラーメン",
            "떡볶이",
            "crème",
            "吃到飽",
            "太陽餅",
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
    fn validate_rejects_invisible_and_combining_characters() {
        for text in [
            "\u{3164}",
            "火\u{200B}鍋",
            "a\u{0345}\u{0363}",
            "\u{05D0}\u{05D1}",
            "火\u{202E}鍋",
        ] {
            assert_eq!(
                validate_food(text),
                Err(FoodRejection::UnsafeCharacter),
                "{text:?}"
            );
        }
    }

    #[test]
    fn validate_rejects_non_food() {
        for text in [
            "藥",
            "感冒藥",
            "屎",
            "醋",
            "土",
            "什麼藥",
            "哪裡",
            "很飽",
            "這個",
            "多少",
            "幾個",
            "到底",
            "還要",
            "先",
            "得下",
            "好的",
            "屁",
            "多一點",
            "a",
            "1",
        ] {
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
    fn looks_like_food_needs_a_dish_hint() {
        assert!(looks_like_food("火鍋"));
        assert!(looks_like_food("麥當勞"));
        assert!(!looks_like_food("看電影"));
        assert!(!looks_like_food("晴天"));
        assert!(!looks_like_food(""));
    }

    #[test]
    fn parse_options_splits_and_dedupes() {
        assert_eq!(
            parse_options("火鍋還是拉麵"),
            Ok(strings(&["火鍋", "拉麵"]))
        );
        assert_eq!(
            parse_options("火鍋 拉麵、壽司/火鍋"),
            Ok(strings(&["火鍋", "拉麵", "壽司"]))
        );
        assert_eq!(
            parse_options("火鍋還是吃拉麵呢"),
            Ok(strings(&["火鍋", "拉麵"]))
        );
        assert_eq!(
            parse_options("要吃火鍋還是要吃拉麵"),
            Ok(strings(&["火鍋", "拉麵"]))
        );
    }

    #[test]
    fn parse_options_drops_generic_and_declining_parts() {
        assert_eq!(parse_options("火鍋還是什麼"), Ok(strings(&["火鍋"])));
        assert_eq!(parse_options("火鍋還是不要"), Ok(strings(&["火鍋"])));
        assert_eq!(parse_options("火鍋或隨便"), Ok(strings(&["火鍋"])));
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
