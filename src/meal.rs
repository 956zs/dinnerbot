//! Meal slots and how to recognise them in chat.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Meal {
    Breakfast,
    Lunch,
    Dinner,
    LateNight,
}

/// Keywords checked in order; longer phrases come before their substrings.
const MEAL_KEYWORDS: &[(&str, Meal)] = &[
    ("早午餐", Meal::Lunch),
    ("早餐", Meal::Breakfast),
    ("早飯", Meal::Breakfast),
    ("早上", Meal::Breakfast),
    ("午餐", Meal::Lunch),
    ("午飯", Meal::Lunch),
    ("中餐", Meal::Lunch),
    ("中午", Meal::Lunch),
    ("晚餐", Meal::Dinner),
    ("晚飯", Meal::Dinner),
    ("晚上", Meal::Dinner),
    ("今晚", Meal::Dinner),
    ("宵夜", Meal::LateNight),
    ("消夜", Meal::LateNight),
    ("半夜", Meal::LateNight),
];

impl Meal {
    pub const ALL: [Meal; 4] = [Meal::Breakfast, Meal::Lunch, Meal::Dinner, Meal::LateNight];

    /// The meal people are most likely asking about at a local hour (0-23).
    pub fn from_hour(hour: u32) -> Meal {
        match hour {
            5..=9 => Meal::Breakfast,
            10..=13 => Meal::Lunch,
            14..=20 => Meal::Dinner,
            _ => Meal::LateNight,
        }
    }

    /// Finds the first meal keyword mentioned in `text`.
    pub fn detect(text: &str) -> Option<Meal> {
        MEAL_KEYWORDS
            .iter()
            .filter_map(|(keyword, meal)| text.find(keyword).map(|index| (index, *meal)))
            .min_by_key(|(index, _)| *index)
            .map(|(_, meal)| meal)
    }

    pub fn label(self) -> &'static str {
        match self {
            Meal::Breakfast => "早餐",
            Meal::Lunch => "午餐",
            Meal::Dinner => "晚餐",
            Meal::LateNight => "宵夜",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hour_covers_every_hour() {
        assert_eq!(Meal::from_hour(5), Meal::Breakfast);
        assert_eq!(Meal::from_hour(9), Meal::Breakfast);
        assert_eq!(Meal::from_hour(10), Meal::Lunch);
        assert_eq!(Meal::from_hour(13), Meal::Lunch);
        assert_eq!(Meal::from_hour(14), Meal::Dinner);
        assert_eq!(Meal::from_hour(20), Meal::Dinner);
        assert_eq!(Meal::from_hour(21), Meal::LateNight);
        assert_eq!(Meal::from_hour(0), Meal::LateNight);
        assert_eq!(Meal::from_hour(4), Meal::LateNight);
        // Out-of-range input still lands somewhere sensible.
        assert_eq!(Meal::from_hour(99), Meal::LateNight);
    }

    #[test]
    fn detect_finds_keywords() {
        assert_eq!(Meal::detect("晚餐吃什麼"), Some(Meal::Dinner));
        assert_eq!(Meal::detect("宵夜要吃啥"), Some(Meal::LateNight));
        assert_eq!(Meal::detect("早午餐去哪"), Some(Meal::Lunch));
        assert_eq!(Meal::detect("明天早上吃什麼"), Some(Meal::Breakfast));
    }

    #[test]
    fn detect_prefers_the_earliest_keyword() {
        assert_eq!(Meal::detect("中午還是晚上"), Some(Meal::Lunch));
    }

    #[test]
    fn detect_returns_none_without_keywords() {
        assert_eq!(Meal::detect(""), None);
        assert_eq!(Meal::detect("吃什麼"), None);
    }
}
