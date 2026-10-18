//! The food menu and reply templates, loaded from `data/data.json`.

use std::fmt;

use serde::Deserialize;

use crate::meal::Meal;
use crate::rng::{self, RandomSource};
use crate::text;

/// The menu shipped with the binary.
const EMBEDDED: &str = include_str!("../data/data.json");

/// Placeholder that templates replace with the food name.
pub const FOOD_PLACEHOLDER: &str = "{food}";

const MAX_FOOD_DISPLAY_CHARS: usize = 32;
const MAX_TEMPLATE_CHARS: usize = 200;

/// How often a recommendation re-rolls to avoid a recently suggested food.
const AVOID_ATTEMPTS: usize = 8;

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
    ZeroWeight(String),
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
            DataError::ZeroWeight(name) => write!(f, "category \"{name}\" has weight 0"),
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
            if category.weight == 0 {
                return Err(DataError::ZeroWeight(category.name.clone()));
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

    /// Picks a food for `meal`: first a category by weight, then a food in it.
    ///
    /// Foods in `avoid` are re-rolled a few times so a channel does not see the
    /// same suggestion twice in a row. Categories that do not serve `meal` are
    /// skipped unless none do.
    pub fn pick_food(&self, meal: Meal, avoid: &[String], rng: &mut impl RandomSource) -> &str {
        let serving: Vec<&Category> = self
            .categories
            .iter()
            .filter(|category| category.meals.contains(&meal))
            .collect();
        let candidates: Vec<&Category> = if serving.is_empty() {
            self.categories.iter().collect()
        } else {
            serving
        };

        let mut pick = || -> &str {
            let category = rng::pick_weighted(rng, &candidates, |category| category.weight)
                .expect("validated data has a category with non-zero weight");
            rng::pick(rng, &category.foods).expect("validated categories are non-empty")
        };

        let mut food = pick();
        for _ in 1..AVOID_ATTEMPTS {
            if !avoid.iter().any(|recent| recent == food) {
                break;
            }
            food = pick();
        }
        food
    }
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
            Err(DataError::ZeroWeight(_))
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

    #[test]
    fn pick_food_respects_meals() {
        let data = sample();
        let mut rng = SplitMix64::new(7);
        for _ in 0..50 {
            let breakfast = data.pick_food(Meal::Breakfast, &[], &mut rng);
            assert!(["蛋餅", "飯糰"].contains(&breakfast));
            assert_eq!(data.pick_food(Meal::Dinner, &[], &mut rng), "火鍋");
        }
    }

    #[test]
    fn pick_food_falls_back_when_no_category_serves_the_meal() {
        let data = sample();
        let lunch = data.pick_food(Meal::Lunch, &[], &mut ScriptedRng::zeros());
        assert_eq!(lunch, "蛋餅");
    }

    #[test]
    fn pick_food_avoids_recent_foods_when_possible() {
        let data = sample();
        let avoid = vec!["蛋餅".to_owned()];
        let mut rng = SplitMix64::new(1);
        for _ in 0..50 {
            assert_eq!(data.pick_food(Meal::Breakfast, &avoid, &mut rng), "飯糰");
        }
        // With a single option there is nothing else to pick.
        assert_eq!(
            data.pick_food(Meal::Dinner, &["火鍋".to_owned()], &mut rng),
            "火鍋"
        );
    }
}
