require("dotenv").config();

const {
  Client,
  Events,
  GatewayIntentBits,
  Partials,
  PermissionFlagsBits
} = require("discord.js");
const data = require("../data/data.json");
const { buildDinnerReply } = require("./dinner8ball");

const token = process.env.DISCORD_TOKEN;
const deleteReactionEmoji = "❌";
const replyOwners = new Map();

async function getReplyOwnerId(message) {
  const cachedOwnerId = replyOwners.get(message.id);

  if (cachedOwnerId) {
    return cachedOwnerId;
  }

  const referenceMessageId = message.reference?.messageId;

  if (!referenceMessageId) {
    return null;
  }

  try {
    const referencedMessage = await message.channel.messages.fetch(referenceMessageId);
    return referencedMessage.author?.id ?? null;
  } catch (error) {
    console.error("Failed to fetch referenced message for delete reaction:", error);
    return null;
  }
}

async function hasAdministratorPermission(message, userId) {
  if (!message.guild) {
    return false;
  }

  try {
    const member = await message.guild.members.fetch(userId);
    return member.permissions.has(PermissionFlagsBits.Administrator);
  } catch (error) {
    console.error("Failed to fetch member for delete reaction:", error);
    return false;
  }
}

if (!token) {
  console.error("Missing DISCORD_TOKEN. Copy .env.example to .env and set your bot token.");
  process.exit(1);
}

const client = new Client({
  intents: [
    GatewayIntentBits.Guilds,
    GatewayIntentBits.GuildMessages,
    GatewayIntentBits.GuildMessageReactions,
    GatewayIntentBits.MessageContent
  ],
  partials: [
    Partials.Message,
    Partials.Channel,
    Partials.Reaction,
    Partials.User
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
    const replyMessage = await message.reply(reply);
    replyOwners.set(replyMessage.id, message.author.id);

    try {
      await replyMessage.react(deleteReactionEmoji);
    } catch (error) {
      console.error("Failed to add delete reaction:", error);
    }
  } catch (error) {
    console.error("Failed to reply to message:", error);
  }
});

client.on(Events.MessageReactionAdd, async (reaction, user) => {
  try {
    if (user.partial) {
      await user.fetch();
    }

    if (user.bot || reaction.emoji.name !== deleteReactionEmoji) {
      return;
    }

    if (reaction.partial) {
      await reaction.fetch();
    }

    const message = reaction.message.partial
      ? await reaction.message.fetch()
      : reaction.message;

    if (message.author?.id !== client.user.id) {
      return;
    }

    const isAdministrator = await hasAdministratorPermission(message, user.id);

    if (!isAdministrator) {
      const ownerId = await getReplyOwnerId(message);

      if (ownerId !== user.id) {
        return;
      }
    }

    if (!message.deletable) {
      return;
    }

    await message.delete();
    replyOwners.delete(message.id);
  } catch (error) {
    console.error("Failed to delete reply from reaction:", error);
  }
});

client.on(Events.MessageDelete, (message) => {
  replyOwners.delete(message.id);
});

client.login(token);
