//! The food menu and reply templates, loaded from `data/data.json`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::meal::Meal;
use crate::rng::{self, RandomSource};
use crate::text;

/// The menu shipped with the binary.
const EMBEDDED: &str = include_str!("../data/data.json");

/// Placeholder that templates replace with the food name.
pub const FOOD_PLACEHOLDER: &str = "{food}";

const MAX_FOOD_DISPLAY_CHARS: usize = 32;
/// Largest category weight; keeps weight sums far from overflowing.
pub const MAX_WEIGHT: u32 = 1_000_000;
const MAX_TEMPLATE_CHARS: usize = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodData {
    pub categories: Vec<Category>,
    pub responses: Responses,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub name: String,
    pub meals: Vec<Meal>,
    #[serde(default = "default_weight")]
    pub weight: u32,
    pub foods: Vec<String>,
}

fn default_weight() -> u32 {
    10
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Responses {
    pub positive: Vec<String>,
    pub neutral: Vec<String>,
    pub negative: Vec<String>,
    pub choose: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    Parse(String),
    NoCategories,
    EmptyCategory(String),
    NoMeals(String),
    BadWeight(String),
    BadFood {
        category: String,
        food: String,
    },
    EmptyTemplates(&'static str),
    BadTemplate {
        list: &'static str,
        template: String,
    },
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataError::Parse(reason) => write!(f, "menu data is not valid JSON for the expected schema: {reason}"),
            DataError::NoCategories => write!(f, "menu data has no categories"),
            DataError::EmptyCategory(name) => write!(f, "category \"{name}\" has no name or no foods"),
            DataError::NoMeals(name) => write!(f, "category \"{name}\" lists no meals"),
            DataError::BadWeight(name) => {
                write!(f, "category \"{name}\" needs a weight from 1 to {MAX_WEIGHT}")
            }
            DataError::BadFood { category, food } => {
                write!(f, "category \"{category}\" has an unusable food name: {food:?}")
            }
            DataError::EmptyTemplates(list) => write!(f, "response list \"{list}\" is empty"),
            DataError::BadTemplate { list, template } => write!(
                f,
                "response \"{template}\" in \"{list}\" must contain {FOOD_PLACEHOLDER} exactly once and no markup"
            ),
        }
    }
}

impl std::error::Error for DataError {}

impl FoodData {
    /// Parses and validates menu JSON.
    pub fn from_json(json: &str) -> Result<Self, DataError> {
        let data: FoodData =
            serde_json::from_str(json).map_err(|error| DataError::Parse(error.to_string()))?;
        data.validate()?;
        Ok(data)
    }

    /// The menu compiled into the binary.
    pub fn embedded() -> Result<Self, DataError> {
        Self::from_json(EMBEDDED)
    }

    fn validate(&self) -> Result<(), DataError> {
        if self.categories.is_empty() {
            return Err(DataError::NoCategories);
        }
        for category in &self.categories {
            if category.name.trim().is_empty() || category.foods.is_empty() {
                return Err(DataError::EmptyCategory(category.name.clone()));
            }
            if category.meals.is_empty() {
                return Err(DataError::NoMeals(category.name.clone()));
            }
            if !(1..=MAX_WEIGHT).contains(&category.weight) {
                return Err(DataError::BadWeight(category.name.clone()));
            }
            if let Some(food) = category
                .foods
                .iter()
                .find(|food| !text::is_safe_display_text(food, MAX_FOOD_DISPLAY_CHARS))
            {
                return Err(DataError::BadFood {
                    category: category.name.clone(),
                    food: food.clone(),
                });
            }
        }

        let responses = &self.responses;
        for (list, templates) in [
            ("positive", &responses.positive),
            ("neutral", &responses.neutral),
            ("negative", &responses.negative),
            ("choose", &responses.choose),
        ] {
            if templates.is_empty() {
                return Err(DataError::EmptyTemplates(list));
            }
            if let Some(template) = templates
                .iter()
                .find(|template| !is_valid_template(template))
            {
                return Err(DataError::BadTemplate {
                    list,
                    template: template.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn food_count(&self) -> usize {
        self.categories
            .iter()
            .map(|category| category.foods.len())
            .sum()
    }

    /// Picks a food: first a pool by weight, then a food in it.
    ///
    /// Built-in categories that do not serve the meal are skipped unless none
    /// do. Foods containing an avoided keyword are never picked; recently
    /// suggested foods are skipped while anything else is left. Returns `None`
    /// only when the avoided keywords rule out everything.
    pub fn pick_food<'a>(
        &'a self,
        rules: &PickRules<'a>,
        rng: &mut impl RandomSource,
    ) -> Option<&'a str> {
        let pools = self.pools(rules);
        let fresh: Vec<Pool<'a>> = pools
            .iter()
            .map(|pool| Pool {
                weight: pool.weight,
                foods: pool
                    .foods
                    .iter()
                    .copied()
                    .filter(|food| !rules.recent.iter().any(|recent| recent == food))
                    .collect(),
            })
            .filter(|pool| !pool.foods.is_empty())
            .collect();
        let source = if fresh.is_empty() { &pools } else { &fresh };

        let pool = rng::pick_weighted(rng, source, |pool| pool.weight)?;
        rng::pick(rng, &pool.foods).copied()
    }

    fn pools<'a>(&'a self, rules: &PickRules<'a>) -> Vec<Pool<'a>> {
        let allowed = |food: &&str| !is_avoided(food, rules.avoid);

        let serving: Vec<&Category> = self
            .categories
            .iter()
            .filter(|category| category.meals.contains(&rules.meal))
            .collect();
        let categories = if serving.is_empty() {
            self.categories.iter().collect()
        } else {
            serving
        };
        let mut pools: Vec<Pool<'a>> = categories
            .into_iter()
            .map(|category| Pool {
                weight: category.weight,
                foods: category
                    .foods
                    .iter()
                    .map(String::as_str)
                    .filter(allowed)
                    .collect(),
            })
            .filter(|pool| !pool.foods.is_empty())
            .collect();

        let custom: Vec<&str> = rules
            .custom
            .iter()
            .map(String::as_str)
            .filter(allowed)
            .collect();
        match rules.mode {
            MenuMode::BuiltinOnly => {}
            _ if custom.is_empty() => {}
            MenuMode::CustomOnly => {
                pools = vec![Pool {
                    weight: 1,
                    foods: custom,
                }];
            }
            MenuMode::Mixed => {
                // The server's own foods get as much weight as the whole
                // built-in menu, so about half of all picks.
                let builtin: u64 = pools.iter().map(|pool| u64::from(pool.weight)).sum();
                pools.push(Pool {
                    weight: u32::try_from(builtin).unwrap_or(u32::MAX).max(1),
                    foods: custom,
                });
            }
        }
        pools
    }
}

/// What a recommendation may draw from.
#[derive(Debug, Clone, Copy)]
pub struct PickRules<'a> {
    pub meal: Meal,
    /// Foods suggested recently in the channel.
    pub recent: &'a [String],
    /// The server's own foods.
    pub custom: &'a [String],
    pub mode: MenuMode,
    /// Keywords the asker avoids, such as "牛" or "辣".
    pub avoid: &'a [String],
}

impl PickRules<'_> {
    /// Only the built-in menu, no history and no preferences.
    pub fn plain(meal: Meal) -> Self {
        Self {
            meal,
            recent: &[],
            custom: &[],
            mode: MenuMode::Mixed,
            avoid: &[],
        }
    }
}

/// Where recommendations come from in a server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuMode {
    /// The server's own foods and the built-in menu, about half and half.
    #[default]
    Mixed,
    /// Only the server's own foods (the built-in menu while it has none).
    CustomOnly,
    /// Only the built-in menu.
    BuiltinOnly,
}

struct Pool<'a> {
    weight: u32,
    foods: Vec<&'a str>,
}

/// The first avoided keyword found in `food`, if any.
/// Matching ignores case and full-width letters, so "ikea" catches "IKEA肉丸".
pub fn avoided_keyword<'k>(food: &str, avoid: &'k [String]) -> Option<&'k str> {
    let food = text::fold(food);
    avoid
        .iter()
        .map(String::as_str)
        .find(|keyword| !keyword.is_empty() && food.contains(&text::fold(keyword)))
}

pub fn is_avoided(food: &str, avoid: &[String]) -> bool {
    avoided_keyword(food, avoid).is_some()
}

fn is_valid_template(template: &str) -> bool {
    template.matches(FOOD_PLACEHOLDER).count() == 1
        && text::is_safe_display_text(&template.replace(FOOD_PLACEHOLDER, ""), MAX_TEMPLATE_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::tests::ScriptedRng;
    use crate::rng::SplitMix64;

    const SAMPLE: &str = r#"{
        "categories": [
            { "name": "早點", "meals": ["breakfast"], "foods": ["蛋餅", "飯糰"] },
            { "name": "晚餐", "meals": ["dinner", "late_night"], "weight": 5, "foods": ["火鍋"] }
        ],
        "responses": {
            "positive": ["吃{food}！"],
            "neutral": ["{food}還行。"],
            "negative": ["先別吃{food}。"],
            "choose": ["選{food}。"]
        }
    }"#;

    fn sample() -> FoodData {
        FoodData::from_json(SAMPLE).expect("sample data is valid")
    }

    fn with(json_edit: impl Fn(&mut serde_json::Value)) -> Result<FoodData, DataError> {
        let mut value: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        json_edit(&mut value);
        FoodData::from_json(&value.to_string())
    }

    #[test]
    fn embedded_menu_is_valid_and_complete() {
        let data = FoodData::embedded().expect("embedded data must validate");
        assert_eq!(data.food_count(), 310);
        for meal in Meal::ALL {
            assert!(
                data.categories
                    .iter()
                    .any(|category| category.meals.contains(&meal)),
                "no category serves {meal:?}"
            );
        }
    }

    #[test]
    fn defaults_weight_to_ten() {
        let data = sample();
        assert_eq!(data.categories[0].weight, 10);
        assert_eq!(data.categories[1].weight, 5);
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(FoodData::from_json("{"), Err(DataError::Parse(_))));
        assert!(matches!(FoodData::from_json(""), Err(DataError::Parse(_))));
        assert!(matches!(
            with(|v| {
                v["extra"] = 1.into();
            }),
            Err(DataError::Parse(_))
        ));
        assert!(matches!(
            with(|v| {
                v["categories"][0]["meals"][0] = "brunch".into();
            }),
            Err(DataError::Parse(_))
        ));
    }

    #[test]
    fn rejects_structural_problems() {
        assert_eq!(
            with(|v| v["categories"] = serde_json::json!([])).unwrap_err(),
            DataError::NoCategories
        );
        assert!(matches!(
            with(|v| v["categories"][0]["foods"] = serde_json::json!([])),
            Err(DataError::EmptyCategory(_))
        ));
        assert!(matches!(
            with(|v| v["categories"][0]["meals"] = serde_json::json!([])),
            Err(DataError::NoMeals(_))
        ));
        assert!(matches!(
            with(|v| v["categories"][0]["weight"] = 0.into()),
            Err(DataError::BadWeight(_))
        ));
    }

    #[test]
    fn rejects_unsafe_foods_and_templates() {
        assert!(matches!(
            with(|v| v["categories"][0]["foods"][0] = "@everyone".into()),
            Err(DataError::BadFood { .. })
        ));
        assert!(matches!(
            with(|v| v["responses"]["choose"] = serde_json::json!([])),
            Err(DataError::EmptyTemplates("choose"))
        ));
        for template in ["沒有佔位符", "{food}{food}", "<@1> {food}"] {
            assert!(
                matches!(
                    with(|v| v["responses"]["positive"][0] = template.into()),
                    Err(DataError::BadTemplate {
                        list: "positive",
                        ..
                    })
                ),
                "{template}"
            );
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn pick_food_respects_meals() {
        let data = sample();
        let mut rng = SplitMix64::new(7);
        for _ in 0..50 {
            let breakfast = data
                .pick_food(&PickRules::plain(Meal::Breakfast), &mut rng)
                .unwrap();
            assert!(["蛋餅", "飯糰"].contains(&breakfast));
            let dinner = data.pick_food(&PickRules::plain(Meal::Dinner), &mut rng);
            assert_eq!(dinner, Some("火鍋"));
        }
    }

    #[test]
    fn pick_food_falls_back_when_no_category_serves_the_meal() {
        let data = sample();
        let lunch = data.pick_food(&PickRules::plain(Meal::Lunch), &mut ScriptedRng::zeros());
        assert_eq!(lunch, Some("蛋餅"));
    }

    #[test]
    fn pick_food_skips_recent_foods_while_others_remain() {
        let data = sample();
        let recent = strings(&["蛋餅"]);
        let rules = PickRules {
            recent: &recent,
            ..PickRules::plain(Meal::Breakfast)
        };
        let mut rng = SplitMix64::new(1);
        for _ in 0..50 {
            assert_eq!(data.pick_food(&rules, &mut rng), Some("飯糰"));
        }
        // With a single option there is nothing else to pick.
        let recent = strings(&["火鍋"]);
        let rules = PickRules {
            recent: &recent,
            ..PickRules::plain(Meal::Dinner)
        };
        assert_eq!(data.pick_food(&rules, &mut rng), Some("火鍋"));
    }

    #[test]
    fn pick_food_never_picks_avoided_foods() {
        let data = sample();
        let avoid = strings(&["蛋"]);
        let rules = PickRules {
            avoid: &avoid,
            ..PickRules::plain(Meal::Breakfast)
        };
        let mut rng = SplitMix64::new(2);
        for _ in 0..50 {
            assert_eq!(data.pick_food(&rules, &mut rng), Some("飯糰"));
        }
        let avoid = strings(&["火"]);
        let rules = PickRules {
            avoid: &avoid,
            ..PickRules::plain(Meal::Dinner)
        };
        assert_eq!(data.pick_food(&rules, &mut rng), None);
    }

    #[test]
    fn custom_menu_modes() {
        let data = sample();
        let custom = strings(&["巷口牛肉麵"]);
        let mut rng = SplitMix64::new(3);

        let only = PickRules {
            custom: &custom,
            mode: MenuMode::CustomOnly,
            ..PickRules::plain(Meal::Dinner)
        };
        for _ in 0..20 {
            assert_eq!(data.pick_food(&only, &mut rng), Some("巷口牛肉麵"));
        }

        let builtin = PickRules {
            custom: &custom,
            mode: MenuMode::BuiltinOnly,
            ..PickRules::plain(Meal::Dinner)
        };
        for _ in 0..20 {
            assert_eq!(data.pick_food(&builtin, &mut rng), Some("火鍋"));
        }

        let mixed = PickRules {
            custom: &custom,
            ..PickRules::plain(Meal::Dinner)
        };
        let picks: Vec<&str> = (0..200)
            .filter_map(|_| data.pick_food(&mixed, &mut rng))
            .collect();
        let custom_share = picks.iter().filter(|food| **food == "巷口牛肉麵").count();
        assert!(
            (60..=140).contains(&custom_share),
            "custom share was {custom_share}/200"
        );
    }

    #[test]
    fn custom_only_falls_back_when_custom_is_empty_or_avoided() {
        let data = sample();
        let rules = PickRules {
            mode: MenuMode::CustomOnly,
            ..PickRules::plain(Meal::Dinner)
        };
        assert_eq!(
            data.pick_food(&rules, &mut ScriptedRng::zeros()),
            Some("火鍋")
        );

        let custom = strings(&["牛肉麵"]);
        let avoid = strings(&["牛"]);
        let rules = PickRules {
            custom: &custom,
            avoid: &avoid,
            mode: MenuMode::CustomOnly,
            ..PickRules::plain(Meal::Dinner)
        };
        assert_eq!(
            data.pick_food(&rules, &mut ScriptedRng::zeros()),
            Some("火鍋")
        );
    }

    #[test]
    fn avoided_keyword_reports_the_match() {
        let avoid = strings(&["牛", "辣"]);
        assert_eq!(avoided_keyword("麻辣燙", &avoid), Some("辣"));
        assert_eq!(avoided_keyword("牛肉麵", &avoid), Some("牛"));
        assert_eq!(avoided_keyword("拉麵", &avoid), None);
        assert_eq!(avoided_keyword("拉麵", &strings(&[""])), None);
        assert_eq!(avoided_keyword("拉麵", &[]), None);
    }

    #[test]
    fn avoided_keyword_ignores_case_and_width() {
        assert_eq!(
            avoided_keyword("IKEA肉丸", &strings(&["ikea"])),
            Some("ikea")
        );
        assert_eq!(
            avoided_keyword("ikea肉丸", &strings(&["IKEA"])),
            Some("IKEA")
        );
        assert_eq!(
            avoided_keyword("ＩＫＥＡ肉丸", &strings(&["ikea"])),
            Some("ikea")
        );
    }

    #[test]
    fn huge_weights_are_rejected() {
        assert!(matches!(
            with(|v| v["categories"][0]["weight"] = (MAX_WEIGHT + 1).into()),
            Err(DataError::BadWeight(_))
        ));
        assert!(with(|v| v["categories"][0]["weight"] = MAX_WEIGHT.into()).is_ok());
    }
}
