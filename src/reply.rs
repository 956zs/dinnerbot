//! Builds the bot's answer for a parsed [`Intent`].

use crate::data::{self, FoodData, MenuMode, PickRules, FOOD_PLACEHOLDER};
use crate::intent::Intent;
use crate::meal::Meal;
use crate::rng::{self, stable_hash, RandomSource, SplitMix64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Positive,
    Neutral,
    Negative,
}

/// Verdicts lean positive but can say no.
const VERDICT_MOODS: [(Mood, u32); 3] =
    [(Mood::Positive, 5), (Mood::Neutral, 3), (Mood::Negative, 2)];

/// Recommending something and then advising against it would be odd.
const RECOMMEND_MOODS: [(Mood, u32); 2] = [(Mood::Positive, 1), (Mood::Neutral, 1)];

/// Reply when every food is ruled out by the asker's avoided keywords.
pub const NOTHING_LEFT: &str = "你的忌口把能吃的都排除了，用 /忌口 調整一下吧。";

/// Who is asking, and on which local day. Verdicts and choices are fixed per
/// asker per day: asking the 8-ball again does not change your fate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asker {
    pub user_id: u64,
    pub day: i64,
}

/// Everything around a question that shapes the answer.
#[derive(Debug, Clone, Copy)]
pub struct Situation<'a> {
    pub asker: Asker,
    /// Meal to assume when the question names none.
    pub default_meal: Meal,
    /// Foods suggested recently in the channel.
    pub recent: &'a [String],
    /// The server's own foods.
    pub custom: &'a [String],
    pub mode: MenuMode,
    /// Keywords the asker avoids.
    pub avoid: &'a [String],
}

impl<'a> Situation<'a> {
    /// No history, no server menu and no preferences.
    pub fn plain(asker: Asker, default_meal: Meal) -> Self {
        Self {
            asker,
            default_meal,
            recent: &[],
            custom: &[],
            mode: MenuMode::Mixed,
            avoid: &[],
        }
    }

    pub fn pick_rules(&self, meal: Option<Meal>) -> PickRules<'a> {
        PickRules {
            meal: meal.unwrap_or(self.default_meal),
            recent: self.recent,
            custom: self.custom,
            mode: self.mode,
            avoid: self.avoid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    /// The food the bot picked by itself, if any. Used to avoid repeats.
    pub recommended: Option<String>,
}

impl Reply {
    fn text(text: String) -> Self {
        Self {
            text,
            recommended: None,
        }
    }
}

pub fn respond(
    intent: &Intent,
    data: &FoodData,
    situation: &Situation<'_>,
    rng: &mut impl RandomSource,
) -> Reply {
    match intent {
        Intent::Recommend { meal } => recommend(data, &situation.pick_rules(*meal), rng),
        Intent::Judge { food, .. } => {
            Reply::text(judge(data, food, situation.asker, situation.avoid))
        }
        Intent::Choose { options, .. } => {
            Reply::text(choose(data, options, situation.asker, situation.avoid))
        }
    }
}

pub fn recommend(data: &FoodData, rules: &PickRules<'_>, rng: &mut impl RandomSource) -> Reply {
    let Some(food) = data.pick_food(rules, rng) else {
        return Reply::text(NOTHING_LEFT.to_owned());
    };
    let mood = pick_mood(rng, &RECOMMEND_MOODS);
    let template = pick_template(rng, templates(data, mood));
    Reply {
        text: fill(template, food),
        recommended: Some(food.to_owned()),
    }
}

/// A verdict on one food, fixed for this asker, food and day. Foods the
/// asker avoids always get talked down.
pub fn judge(data: &FoodData, food: &str, asker: Asker, avoid: &[String]) -> String {
    if let Some(keyword) = data::avoided_keyword(food, avoid) {
        return format!("你不是說不吃{keyword}嗎？{food}還是算了吧。");
    }
    let mut fate = fate_rng("judge", asker, &[&food.to_lowercase()]);
    let mood = pick_mood(&mut fate, &VERDICT_MOODS);
    fill(pick_template(&mut fate, templates(data, mood)), food)
}

/// Picks one option, fixed for this asker, option set and day. Options the
/// asker avoids are left out unless that would leave nothing.
///
/// # Panics
///
/// Panics if `options` is empty; the parser never produces that.
pub fn choose(data: &FoodData, options: &[String], asker: Asker, avoid: &[String]) -> String {
    let edible: Vec<&String> = options
        .iter()
        .filter(|option| !data::is_avoided(option, avoid))
        .collect();
    let mut sorted = if edible.is_empty() {
        options.iter().collect()
    } else {
        edible
    };
    // Sorted so "火鍋還是拉麵" and "拉麵還是火鍋" share a fate.
    sorted.sort_by_key(|option| option.to_lowercase());
    let key: Vec<String> = sorted.iter().map(|option| option.to_lowercase()).collect();
    let key: Vec<&str> = key.iter().map(String::as_str).collect();

    let mut fate = fate_rng("choose", asker, &key);
    let chosen = *rng::pick(&mut fate, &sorted).expect("choose needs at least one option");
    fill(pick_template(&mut fate, &data.responses.choose), chosen)
}

fn fate_rng(kind: &str, asker: Asker, parts: &[&str]) -> SplitMix64 {
    let user = asker.user_id.to_le_bytes();
    let day = asker.day.to_le_bytes();
    let mut bytes: Vec<&[u8]> = vec![kind.as_bytes(), &user, &day];
    bytes.extend(parts.iter().map(|part| part.as_bytes()));
    SplitMix64::new(stable_hash(&bytes))
}

fn pick_mood(rng: &mut impl RandomSource, moods: &[(Mood, u32)]) -> Mood {
    rng::pick_weighted(rng, moods, |(_, weight)| *weight)
        .map(|(mood, _)| *mood)
        .expect("mood tables have positive weights")
}

fn pick_template<'a>(rng: &mut impl RandomSource, templates: &'a [String]) -> &'a str {
    rng::pick(rng, templates).expect("validated data has templates")
}

fn templates(data: &FoodData, mood: Mood) -> &[String] {
    match mood {
        Mood::Positive => &data.responses.positive,
        Mood::Neutral => &data.responses.neutral,
        Mood::Negative => &data.responses.negative,
    }
}

fn fill(template: &str, food: &str) -> String {
    template.replace(FOOD_PLACEHOLDER, food)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::tests::ScriptedRng;

    fn data() -> FoodData {
        FoodData::from_json(
            r#"{
                "categories": [
                    { "name": "全部", "meals": ["breakfast", "lunch", "dinner", "late_night"], "foods": ["粥", "麵"] }
                ],
                "responses": {
                    "positive": ["吃{food}！", "就{food}。"],
                    "neutral": ["{food}還行。"],
                    "negative": ["先別吃{food}。"],
                    "choose": ["選{food}。", "{food}勝出。"]
                }
            }"#,
        )
        .unwrap()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    const ASKER: Asker = Asker {
        user_id: 42,
        day: 20_000,
    };

    #[test]
    fn recommend_fills_a_positive_or_neutral_template() {
        let rules = PickRules::plain(Meal::Dinner);
        let reply = recommend(&data(), &rules, &mut ScriptedRng::zeros());
        assert_eq!(reply.text, "吃粥！");
        assert_eq!(reply.recommended.as_deref(), Some("粥"));

        let mut rng = SplitMix64::new(3);
        for _ in 0..100 {
            let reply = recommend(&data(), &rules, &mut rng);
            assert!(
                !reply.text.starts_with("先別吃"),
                "recommendations never advise against"
            );
        }
    }

    #[test]
    fn recommend_explains_when_everything_is_avoided() {
        let avoid = strings(&["粥", "麵"]);
        let rules = PickRules {
            avoid: &avoid,
            ..PickRules::plain(Meal::Dinner)
        };
        let reply = recommend(&data(), &rules, &mut ScriptedRng::zeros());
        assert_eq!(reply.text, NOTHING_LEFT);
        assert_eq!(reply.recommended, None);
    }

    #[test]
    fn judge_is_fixed_per_asker_food_and_day() {
        let data = data();
        let first = judge(&data, "火鍋", ASKER, &[]);
        assert_eq!(first, judge(&data, "火鍋", ASKER, &[]));
        assert!(first.contains("火鍋"));
    }

    #[test]
    fn judge_talks_down_avoided_foods() {
        let avoid = strings(&["牛"]);
        assert_eq!(
            judge(&data(), "牛肉麵", ASKER, &avoid),
            "你不是說不吃牛嗎？牛肉麵還是算了吧。"
        );
    }

    #[test]
    fn judge_varies_across_days() {
        let data = data();
        let verdicts: std::collections::HashSet<String> = (0..60)
            .map(|day| judge(&data, "火鍋", Asker { day, ..ASKER }, &[]))
            .collect();
        assert!(verdicts.len() > 1, "fate should change from day to day");
    }

    #[test]
    fn judge_uses_every_mood_over_time() {
        let data = data();
        let verdicts: Vec<String> = (0..300)
            .map(|day| judge(&data, "火鍋", Asker { day, ..ASKER }, &[]))
            .collect();
        assert!(verdicts.iter().any(|v| v.starts_with("先別吃")));
        assert!(verdicts.iter().any(|v| v.ends_with("還行。")));
    }

    #[test]
    fn choose_picks_an_offered_option_regardless_of_order() {
        let data = data();
        let options = strings(&["火鍋", "拉麵"]);
        let reversed = strings(&["拉麵", "火鍋"]);
        let answer = choose(&data, &options, ASKER, &[]);
        assert!(answer.contains("火鍋") || answer.contains("拉麵"));
        assert_eq!(answer, choose(&data, &options, ASKER, &[]));
        // Same option set in another order: same chosen food.
        let chosen = |text: &str| {
            if text.contains("火鍋") {
                "火鍋"
            } else {
                "拉麵"
            }
        };
        assert_eq!(
            chosen(&answer),
            chosen(&choose(&data, &reversed, ASKER, &[]))
        );
    }

    #[test]
    fn choose_skips_avoided_options_unless_all_are() {
        let data = data();
        let options = strings(&["牛排", "拉麵"]);
        let avoid = strings(&["牛"]);
        for day in 0..30 {
            let answer = choose(&data, &options, Asker { day, ..ASKER }, &avoid);
            assert!(answer.contains("拉麵"), "{answer}");
        }
        let everything = strings(&["牛", "麵"]);
        let answer = choose(&data, &options, ASKER, &everything);
        assert!(answer.contains("牛排") || answer.contains("拉麵"));
    }

    #[test]
    #[should_panic(expected = "at least one option")]
    fn choose_panics_without_options() {
        choose(&data(), &[], ASKER, &[]);
    }

    #[test]
    fn respond_dispatches_by_intent() {
        let data = data();
        let mut rng = ScriptedRng::zeros();
        let situation = Situation::plain(ASKER, Meal::Lunch);

        let reply = respond(
            &Intent::Recommend { meal: None },
            &data,
            &situation,
            &mut rng,
        );
        assert!(reply.recommended.is_some());

        let intent = Intent::Judge {
            food: "火鍋".to_owned(),
            meal: None,
        };
        let reply = respond(&intent, &data, &situation, &mut rng);
        assert_eq!(reply.text, judge(&data, "火鍋", ASKER, &[]));
        assert_eq!(reply.recommended, None);

        let options = strings(&["火鍋", "拉麵"]);
        let intent = Intent::Choose {
            options: options.clone(),
            meal: None,
        };
        let reply = respond(&intent, &data, &situation, &mut rng);
        assert_eq!(reply.text, choose(&data, &options, ASKER, &[]));
    }

    #[test]
    fn situation_uses_the_intent_meal_first() {
        let situation = Situation::plain(ASKER, Meal::Lunch);
        assert_eq!(situation.pick_rules(None).meal, Meal::Lunch);
        assert_eq!(
            situation.pick_rules(Some(Meal::LateNight)).meal,
            Meal::LateNight
        );
    }
}
