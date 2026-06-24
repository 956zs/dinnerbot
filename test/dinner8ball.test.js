const test = require("node:test");
const assert = require("node:assert/strict");

const {
  buildDinnerReply,
  extractRequestedFood,
  isGenericFoodQuestion
} = require("../src/dinner8ball");

const testData = {
  foods: ["粥"],
  responses: {
    positive: ["今晚先吃{food}。"],
    neutral: ["{food}也可以。"],
    negative: ["先不要吃{food}。"]
  }
};

test("does not treat generic question fragments as requested food", () => {
  assert.equal(isGenericFoodQuestion("些什麼"), true);
  assert.equal(isGenericFoodQuestion("點什麼"), true);
  assert.equal(isGenericFoodQuestion("一點什麼東西"), true);
  assert.equal(extractRequestedFood("我覺得應該吃些什麼"), null);
});

test("falls back to recommendation when direct pattern ends with a generic question", () => {
  const reply = buildDinnerReply(
    "好吧說實話我現在其實沒有食慾只是我覺得應該吃些什麼",
    testData,
    () => 0
  );

  assert.equal(reply, "今晚先吃粥。");
});

test("still extracts explicitly requested food", () => {
  assert.equal(extractRequestedFood("要不要吃火鍋？"), "火鍋");
  assert.equal(extractRequestedFood("晚餐吃 拉麵 啊？"), "拉麵");
});
