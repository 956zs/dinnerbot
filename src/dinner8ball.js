const triggerKeywords = [
  "晚餐吃什麼",
  "吃什麼",
  "今天吃啥",
  "晚餐吃",
  "要不要吃",
  "該吃",
  "吃啥"
];

const directFoodPatterns = [
  "要不要吃",
  "晚餐吃",
  "今天吃",
  "該吃",
  "吃不吃"
];

const moodPool = [
  "positive",
  "positive",
  "positive",
  "positive",
  "positive",
  "neutral",
  "neutral",
  "neutral",
  "negative",
  "negative"
];

const recommendationMoodPool = [
  "positive",
  "positive",
  "positive",
  "positive",
  "positive",
  "neutral",
  "neutral",
  "neutral",
  "neutral",
  "neutral"
];

const genericFoodQuestions = new Set(["什麼", "甚麼", "什么", "啥", "哪個", "哪一個"]);
const trailingNoisePattern = /[\s?？!！.。,，~～]+$/u;
const fillerPattern = /[\s?？!！.。,，~～]*(嗎|啊|啦|吧|呢)+[\s?？!！.。,，~～]*$/u;

function pickRandom(items, random = Math.random) {
  if (!Array.isArray(items) || items.length === 0) {
    throw new Error("Cannot pick from an empty list.");
  }

  const index = Math.floor(random() * items.length);
  return items[Math.min(index, items.length - 1)];
}

function shouldTrigger(content) {
  return triggerKeywords.some((keyword) => content.includes(keyword));
}

function cleanFoodName(rawFood) {
  let food = rawFood.trim();

  while (fillerPattern.test(food) || trailingNoisePattern.test(food)) {
    food = food.replace(fillerPattern, "").replace(trailingNoisePattern, "").trim();
  }

  return food;
}

function extractRequestedFood(content) {
  for (const pattern of directFoodPatterns) {
    const patternIndex = content.indexOf(pattern);

    if (patternIndex === -1) {
      continue;
    }

    const rawFood = content.slice(patternIndex + pattern.length);
    const food = cleanFoodName(rawFood);

    if (food && !genericFoodQuestions.has(food)) {
      return food;
    }
  }

  return null;
}

function pickMood(random = Math.random) {
  return pickRandom(moodPool, random);
}

function buildDinnerReply(content, data, random = Math.random) {
  if (!shouldTrigger(content)) {
    return null;
  }

  const requestedFood = extractRequestedFood(content);
  const food = requestedFood || pickRandom(data.foods, random);
  const mood = requestedFood ? pickMood(random) : pickRandom(recommendationMoodPool, random);
  const template = pickRandom(data.responses[mood], random);

  return template.replaceAll("{food}", food);
}

module.exports = {
  triggerKeywords,
  directFoodPatterns,
  moodPool,
  recommendationMoodPool,
  shouldTrigger,
  cleanFoodName,
  extractRequestedFood,
  pickMood,
  pickRandom,
  buildDinnerReply
};
