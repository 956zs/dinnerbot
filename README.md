# Dinner 8-Ball Bot

一個用 discord.js 做的「晚餐吃什麼」Discord Bot。

當群友用自然聊天方式問晚餐選擇時，Bot 會像 Magic 8-Ball 一樣，用肯定、中立或否定的語氣回覆。

```text
群友：晚餐吃什麼？
Bot：今天就是「牛肉麵」了，命運已經決定。

群友：要不要吃火鍋？
Bot：「火鍋」正在召喚你。

群友：晚餐吃麥當勞?
Bot：我不建議「麥當勞」，但你要吃也不是不行。
```

## Features

- 使用 discord.js
- 自然語句關鍵字觸發
- 支援指定食物解析，例如 `要不要吃火鍋`、`晚餐吃麥當勞?`
- 未指定食物時從 `data/data.json` 隨機抽取
- 有指定食物時使用肯定、中立、否定三種語氣
- 開放式推薦不使用否定語氣，避免回覆太突兀
- 食物與回覆句型皆以 JSON 管理，方便擴充

## Setup

```bash
npm install
```

複製 `.env.example` 成 `.env`，並填入 Discord bot token：

```env
DISCORD_TOKEN=your_discord_bot_token_here
```

Discord Developer Portal 的 Bot 設定需要開啟 `Message Content Intent`，Bot 才能讀取一般訊息文字。

Bot 也需要可以新增 reaction，才能在回覆上加上 `❌`。使用者按下自己觸發的 bot 回覆上的 `❌` 時，bot 會刪除該則回覆；伺服器 Administrator 也可以用同一個 reaction 刪除 bot 回覆。這是 bot 刪除自己的訊息，不需要 Manage Messages。若要讓 bot 重啟後仍能判斷舊回覆的觸發者，建議同時給予讀取訊息歷史的權限。

## Run

```bash
npm start
```

## Data

食物與回覆句型位於 `data/data.json`。

目前資料以台灣常見晚餐、夜市、便利商店鮮食、連鎖速食與一些聊天梗品項混合而成。資料只保存簡短的食物或商品名稱，沒有收錄品牌 logo、商品圖片、完整菜單截圖、價格表或官方長篇文案。

## Disclaimer

本專案與資料中提及之任何品牌、商標、店家或平台皆無合作、贊助、授權或背書關係。品牌名稱與商品名稱僅作為聊天機器人的趣味參考資料使用。

This project is not affiliated with, endorsed by, sponsored by, or officially connected to any mentioned brands, stores, trademarks, or platforms. Food, product, and brand names are used only as playful reference data for a Discord bot.

公開部署、商業化使用或行銷使用前，建議自行檢查資料內容，並視需求移除品牌名或改成通用食物名稱。

## Notes

- Do not commit `.env` or any Discord bot token.
- Avoid adding brand logos, official images, full menu screenshots, price tables, or copied official copy.
- Keep new food data as short item names where possible.
