// A starting point for "what do other parents block", browsable from the
// list detail screen — not exhaustive, not user-editable (yet): picking a
// domain here is no different from typing it in by hand, it just saves
// having to remember exact domains. Each category/domain stays within
// this file; nothing here reaches the backend until something is actually
// added to a list.
export const SITE_SUGGESTIONS = [
  {
    category: "Réseaux sociaux",
    sites: [
      { domain: "instagram.com", label: "Instagram" },
      { domain: "tiktok.com", label: "TikTok" },
      { domain: "snapchat.com", label: "Snapchat" },
      { domain: "facebook.com", label: "Facebook" },
      { domain: "x.com", label: "X (Twitter)" },
      { domain: "threads.net", label: "Threads" },
      { domain: "pinterest.com", label: "Pinterest" },
      { domain: "bereal.com", label: "BeReal" },
      { domain: "reddit.com", label: "Reddit" },
      { domain: "mastodon.social", label: "Mastodon" },
      { domain: "tumblr.com", label: "Tumblr" },
    ],
  },
  {
    category: "Streaming vidéo",
    sites: [
      { domain: "youtube.com", label: "YouTube" },
      { domain: "netflix.com", label: "Netflix" },
      { domain: "disneyplus.com", label: "Disney+" },
      { domain: "primevideo.com", label: "Prime Video" },
      { domain: "twitch.tv", label: "Twitch" },
      { domain: "dailymotion.com", label: "Dailymotion" },
      { domain: "max.com", label: "Max" },
      { domain: "canalplus.com", label: "Canal+" },
      { domain: "molotov.tv", label: "Molotov" },
      { domain: "vimeo.com", label: "Vimeo" },
    ],
  },
  {
    category: "Jeux",
    sites: [
      { domain: "roblox.com", label: "Roblox" },
      { domain: "fortnite.com", label: "Fortnite" },
      { domain: "epicgames.com", label: "Epic Games" },
      { domain: "minecraft.net", label: "Minecraft" },
      { domain: "steampowered.com", label: "Steam" },
      { domain: "ea.com", label: "EA" },
      { domain: "leagueoflegends.com", label: "League of Legends" },
      { domain: "battle.net", label: "Battle.net" },
      { domain: "poki.com", label: "Poki" },
      { domain: "crazygames.com", label: "CrazyGames" },
    ],
  },
  {
    category: "Messagerie",
    sites: [
      { domain: "whatsapp.com", label: "WhatsApp" },
      { domain: "telegram.org", label: "Telegram" },
      { domain: "discord.com", label: "Discord" },
      { domain: "messenger.com", label: "Messenger" },
      { domain: "signal.org", label: "Signal" },
      { domain: "line.me", label: "Line" },
    ],
  },
  {
    category: "IA et chatbots",
    sites: [
      { domain: "chatgpt.com", label: "ChatGPT" },
      { domain: "claude.ai", label: "Claude" },
      { domain: "gemini.google.com", label: "Gemini" },
      { domain: "copilot.microsoft.com", label: "Copilot" },
      { domain: "meta.ai", label: "Meta AI" },
      { domain: "perplexity.ai", label: "Perplexity" },
      { domain: "grok.com", label: "Grok" },
      { domain: "poe.com", label: "Poe" },
      { domain: "you.com", label: "You.com" },
      { domain: "deepseek.com", label: "DeepSeek" },
      { domain: "mistral.ai", label: "Le Chat (Mistral)" },
      { domain: "huggingface.co", label: "Hugging Face" },
      { domain: "character.ai", label: "Character.AI" },
      { domain: "janitorai.com", label: "JanitorAI" },
      { domain: "crushon.ai", label: "CrushOn.AI" },
      { domain: "replika.com", label: "Replika" },
    ],
  },
  {
    category: "Achats en ligne",
    sites: [
      { domain: "amazon.fr", label: "Amazon" },
      { domain: "aliexpress.com", label: "AliExpress" },
      { domain: "shein.com", label: "Shein" },
      { domain: "vinted.fr", label: "Vinted" },
      { domain: "temu.com", label: "Temu" },
      { domain: "cdiscount.com", label: "Cdiscount" },
      { domain: "leboncoin.fr", label: "Leboncoin" },
    ],
  },
  {
    category: "Paris et jeux d'argent",
    sites: [
      { domain: "winamax.fr", label: "Winamax" },
      { domain: "betclic.fr", label: "Betclic" },
      { domain: "pokerstars.com", label: "PokerStars" },
      { domain: "pmu.fr", label: "PMU" },
      { domain: "parionssport.fdj.fr", label: "Parions Sport" },
    ],
  },
];
