require("dotenv").config();

const { Client, Events, GatewayIntentBits } = require("discord.js");
const data = require("../data/data.json");
const { buildDinnerReply } = require("./dinner8ball");

const token = process.env.DISCORD_TOKEN;

if (!token) {
  console.error("Missing DISCORD_TOKEN. Copy .env.example to .env and set your bot token.");
  process.exit(1);
}

const client = new Client({
  intents: [
    GatewayIntentBits.Guilds,
    GatewayIntentBits.GuildMessages,
    GatewayIntentBits.MessageContent
  ]
});

client.once(Events.ClientReady, (readyClient) => {
  console.log(`Dinner 8-Ball Bot logged in as ${readyClient.user.tag}.`);
});

client.on(Events.MessageCreate, async (message) => {
  if (message.author.bot) {
    return;
  }

  const reply = buildDinnerReply(message.content, data);

  if (!reply) {
    return;
  }

  try {
    await message.reply(reply);
  } catch (error) {
    console.error("Failed to reply to message:", error);
  }
});

client.login(token);
