//! Builds the bot's answer for a parsed [`Intent`].

use crate::data::{FoodData, FOOD_PLACEHOLDER};
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
const VERDICT_MOODS: [(Mood, u32); 3] = [(Mood::Positive, 5), (Mood::Neutral, 3), (Mood::Negative, 2)];

/// Recommending something and then advising against it would be odd.
const RECOMMEND_MOODS: [(Mood, u32); 2] = [(Mood::Positive, 1), (Mood::Neutral, 1)];

/// Who is asking, and on which local day. Verdicts and choices are fixed per
/// asker per day: asking the 8-ball again does not change your fate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asker {
    pub user_id: u64,
    pub day: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    /// The food the bot picked by itself, if any. Used to avoid repeats.
    pub recommended: Option<String>,
}

/// Answers an intent. `default_meal` applies when the message named no meal.
pub fn respond(
    intent: &Intent,
    data: &FoodData,
    asker: Asker,
    default_meal: Meal,
    avoid: &[String],
    rng: &mut impl RandomSource,
) -> Reply {
    match intent {
        Intent::Recommend { meal } => recommend(data, meal.unwrap_or(default_meal), avoid, rng),
        Intent::Judge { food, .. } => Reply {
            text: judge(data, food, asker),
            recommended: None,
        },
        Intent::Choose { options, .. } => Reply {
            text: choose(data, options, asker),
            recommended: None,
        },
    }
}

pub fn recommend(data: &FoodData, meal: Meal, avoid: &[String], rng: &mut impl RandomSource) -> Reply {
    let food = data.pick_food(meal, avoid, rng).to_owned();
    let mood = pick_mood(rng, &RECOMMEND_MOODS);
    let template = pick_template(rng, templates(data, mood));
    Reply {
        text: fill(template, &food),
        recommended: Some(food),
    }
}

/// A verdict on one food, fixed for this asker, food and day.
pub fn judge(data: &FoodData, food: &str, asker: Asker) -> String {
    let mut fate = fate_rng("judge", asker, &[&food.to_lowercase()]);
    let mood = pick_mood(&mut fate, &VERDICT_MOODS);
    fill(pick_template(&mut fate, templates(data, mood)), food)
}

/// Picks one option, fixed for this asker, option set and day.
///
/// # Panics
///
/// Panics if `options` is empty; the parser never produces that.
pub fn choose(data: &FoodData, options: &[String], asker: Asker) -> String {
    // Sorted so "火鍋還是拉麵" and "拉麵還是火鍋" share a fate.
    let mut sorted: Vec<&String> = options.iter().collect();
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

    const ASKER: Asker = Asker { user_id: 42, day: 20_000 };

    #[test]
    fn recommend_fills_a_positive_or_neutral_template() {
        let reply = recommend(&data(), Meal::Dinner, &[], &mut ScriptedRng::zeros());
        assert_eq!(reply.text, "吃粥！");
        assert_eq!(reply.recommended.as_deref(), Some("粥"));

        let mut rng = SplitMix64::new(3);
        for _ in 0..100 {
            let reply = recommend(&data(), Meal::Dinner, &[], &mut rng);
            assert!(!reply.text.starts_with("先別吃"), "recommendations never advise against");
        }
    }

    #[test]
    fn judge_is_fixed_per_asker_food_and_day() {
        let data = data();
        let first = judge(&data, "火鍋", ASKER);
        assert_eq!(first, judge(&data, "火鍋", ASKER));
        assert!(first.contains("火鍋"));
    }

    #[test]
    fn judge_varies_across_days() {
        let data = data();
        let verdicts: std::collections::HashSet<String> = (0..60)
            .map(|day| judge(&data, "火鍋", Asker { day, ..ASKER }))
            .collect();
        assert!(verdicts.len() > 1, "fate should change from day to day");
    }

    #[test]
    fn judge_uses_every_mood_over_time() {
        let data = data();
        let verdicts: Vec<String> = (0..300)
            .map(|day| judge(&data, "火鍋", Asker { day, ..ASKER }))
            .collect();
        assert!(verdicts.iter().any(|v| v.starts_with("先別吃")));
        assert!(verdicts.iter().any(|v| v.ends_with("還行。")));
    }

    #[test]
    fn choose_picks_an_offered_option_regardless_of_order() {
        let data = data();
        let options = vec!["火鍋".to_owned(), "拉麵".to_owned()];
        let reversed = vec!["拉麵".to_owned(), "火鍋".to_owned()];
        let answer = choose(&data, &options, ASKER);
        assert!(answer.contains("火鍋") || answer.contains("拉麵"));
        assert_eq!(answer, choose(&data, &options, ASKER));
        // Same option set in another order: same chosen food.
        let chosen = |text: &str| if text.contains("火鍋") { "火鍋" } else { "拉麵" };
        assert_eq!(chosen(&answer), chosen(&choose(&data, &reversed, ASKER)));
    }

    #[test]
    #[should_panic(expected = "at least one option")]
    fn choose_panics_without_options() {
        choose(&data(), &[], ASKER);
    }

    #[test]
    fn respond_dispatches_by_intent() {
        let data = data();
        let mut rng = ScriptedRng::zeros();

        let reply = respond(&Intent::Recommend { meal: None }, &data, ASKER, Meal::Lunch, &[], &mut rng);
        assert!(reply.recommended.is_some());

        let intent = Intent::Judge { food: "火鍋".to_owned(), meal: None };
        let reply = respond(&intent, &data, ASKER, Meal::Lunch, &[], &mut rng);
        assert_eq!(reply.text, judge(&data, "火鍋", ASKER));
        assert_eq!(reply.recommended, None);

        let options = vec!["火鍋".to_owned(), "拉麵".to_owned()];
        let intent = Intent::Choose { options: options.clone(), meal: None };
        let reply = respond(&intent, &data, ASKER, Meal::Lunch, &[], &mut rng);
        assert_eq!(reply.text, choose(&data, &options, ASKER));
    }
}
