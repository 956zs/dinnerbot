# Dinner 8-Ball Bot

用 Rust 寫的「晚餐吃什麼」Discord Bot。

群友在聊天裡問要吃什麼，Bot 會像 Magic 8-Ball 一樣回答：沒指定食物就幫你挑，有指定就給答案，給了選項就幫你選一個。

```text
群友：晚餐吃什麼？
Bot：就決定是紅燒牛肉麵了，今天真的超適合你！

群友：要不要吃火鍋？
Bot：火鍋嗎？我覺得今天先緩一緩比較好。

群友：宵夜吃鹽酥雞還是滷味
Bot：我選滷味，不接受反駁。
```

## Features

- 純 Rust：serenity + poise，TLS 用 rustls，單一執行檔，菜單直接編進程式
- 三種問法
  - 推薦：`晚餐吃什麼`、`今天吃啥`、`宵夜要吃啥好呢`
  - 判斷：`要不要吃火鍋`、`今天吃火鍋嗎`、`吃不吃火鍋`、`火鍋吃不吃`、`我晚餐吃麥當勞好了`
  - 選擇：`要不要吃火鍋還是拉麵`、`晚餐火鍋還是拉麵`、`宵夜吃鹽酥雞還是滷味或臭豆腐`
- 不亂回
  - 過去式：`你晚餐吃了沒`、`昨天晚餐吃什麼`
  - 只是說要去吃：`我先去吃飯`、`我不吃早餐`
  - 正在吃：`你在吃什麼`
  - 不是食物：`該吃藥了`、`吃醋`、`吃土`
  - 一般陳述與閒聊：`我想吃火鍋`、`這家好吃嗎`、`今天吃很飽`、`今天下雨還是晴天`
- 安全
  - Bot 回覆不會 tag 任何人，只會照一般回覆提醒發問的人。
  - 使用者打的菜名只接受文字、數字和少數符號。菜名裡有 `@`、`<>`、emoji 或 markdown 符號就不回應，訊息含連結也不回應。
- 8-Ball 的命運
  - 同一個人同一天問同一道菜，答案固定，重問也不會變。
  - 同一組選項不管順序怎麼排，選中的都一樣。
- 依時段推薦：沒講哪一餐時看現在幾點，分早餐、午餐、晚餐、宵夜。
- 菜單分類：310 道菜分成 16 類，先抽分類再抽菜，不會被某一類洗版。
- 不重複：同一個頻道最近推薦過的菜會盡量避開。
- 防洗版：每個人有冷卻時間，預設 3 秒。
- 收回回覆
  - 每則回覆都附 `❌`，發問的人按下去就會刪掉。
  - 伺服器管理員、有「管理訊息」權限的人也可以刪。
  - 原本的問題被刪掉時，Bot 的回覆會一起刪掉。
- 斜線指令
  - `/dinner`（中文介面顯示 `/吃什麼`）：可以帶食物和餐別。
  - `/choose`（中文介面顯示 `/幫我選`）：用空白、逗號或「還是」分隔選項。
  - 斜線指令只能在伺服器裡用。

## Setup

1. 到 [Discord Developer Portal](https://discord.com/developers/applications) 建立 Bot，並在 Bot 設定開啟 `Message Content Intent`。
2. 邀請 Bot 時勾選 `bot` 與 `applications.commands` 兩個 scope，權限給：
   - View Channels
   - Send Messages
   - Read Message History：回覆訊息時需要
   - Add Reactions：加上 `❌` 時需要

   不需要 Manage Messages，Bot 只會刪自己的訊息。
3. 安裝 Rust 1.82 以上，然後複製 `.env.example` 成 `.env`，填入 token：

```env
DISCORD_TOKEN=your_discord_bot_token_here
```

## Run

```bash
cargo run --release
```

或先編譯，再把執行檔放到任何地方執行：

```bash
cargo build --release
./target/release/dinnerbot
```

收到 Ctrl+C 或 SIGTERM 時，Bot 會正常斷線再結束。

## Configuration

| 變數 | 預設 | 說明 |
|---|---|---|
| `DISCORD_TOKEN` | 必填 | Bot token |
| `COOLDOWN_SECS` | `3` | 同一個人兩次提問之間的最短秒數，`0` 為關閉，上限 3600 |
| `UTC_OFFSET_HOURS` | `8` | 判斷時段，以及「每天」從幾點算起 |
| `DEV_GUILD_ID` | 無 | 只在這個伺服器註冊斜線指令，會立即生效，適合測試；不填則全域註冊 |
| `DINNERBOT_DATA` | 內建 | 改用外部菜單檔，格式同 `data/data.json` |
| `RUST_LOG` | `info` | 日誌等級，例如 `debug`、`dinnerbot=debug,serenity=warn` |

## Data

食物與回覆句型位於 `data/data.json`，編譯時會直接嵌入程式。啟動時會先檢查格式，有問題就印出原因並結束。

```json
{
  "categories": [
    { "name": "鍋物", "meals": ["dinner", "late_night"], "weight": 10, "foods": ["火鍋", "麻辣燙"] }
  ],
  "responses": {
    "positive": ["就決定是{food}了，今天真的超適合你！"],
    "neutral": ["{food}也不錯啦，可以考慮看看。"],
    "negative": ["今天可能不太適合吃{food}喔。"],
    "choose": ["我選{food}，不接受反駁。"]
  }
}
```

- `meals`：`breakfast`、`lunch`、`dinner`、`late_night`。
- `weight`：分類被抽中的比重，可省略，預設 10。
- 每個句型都必須剛好包含一個 `{food}`。
- 食物名稱與句型都不能有 `@`、`<>`、反引號、markdown 符號或連結。

目前資料以台灣常見晚餐、夜市、便利商店鮮食、連鎖速食與一些聊天梗品項混合而成。資料只保存簡短的食物或商品名稱，沒有收錄品牌 logo、商品圖片、完整菜單截圖、價格表或官方長篇文案。

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

解析、資料驗證、冷卻、回覆紀錄等邏輯都和 Discord 分開，測試不需要連線。

## Disclaimer

本專案與資料中提及之任何品牌、商標、店家或平台皆無合作、贊助、授權或背書關係。品牌名稱與商品名稱僅作為聊天機器人的趣味參考資料使用。

This project is not affiliated with, endorsed by, sponsored by, or officially connected to any mentioned brands, stores, trademarks, or platforms. Food, product, and brand names are used only as playful reference data for a Discord bot.

公開部署、商業化使用或行銷使用前，建議自行檢查資料內容，並視需求移除品牌名或改成通用食物名稱。

## Notes

- 不要 commit `.env` 或任何 Discord bot token。
- 不要加入品牌 logo、官方圖片、完整菜單截圖、價格表或照抄的官方文案。
- 新增食物時盡量只寫簡短的品項名稱。
