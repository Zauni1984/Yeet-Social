// Yeet Social — test-bot runner.
//
// Spins up N deterministic wallet bots, authenticates each via the wallet
// nonce/sign flow (no email, no verification needed), then has them post a
// fresh message and interact with each other (like / comment / follow).
//
// Designed to be run once per day (see .github/workflows/bots.yml). Each run
// is one "day" of activity. The bots are derived deterministically from a
// single seed phrase, so the same accounts persist across runs.
//
// Env:
//   BASE_URL   API + site base (default https://justyeet.it)
//   BOT_SEED   BIP-39 mnemonic the bot wallets are derived from. If unset a
//              built-in default is used so the script works out of the box;
//              set a real secret (GitHub Actions secret BOT_SEED) to keep the
//              bot identities private.
//   BOT_COUNT  how many bots to run (default 5, capped at 25)
//
// No external deps beyond `ethers` (v6) — uses Node's global fetch (Node 18+).

import { Wallet, Mnemonic, HDNodeWallet } from 'ethers';

const BASE_URL = (process.env.BASE_URL || 'https://justyeet.it').replace(/\/$/, '');
// A throwaway default so the runner works without configuration. Override with
// the BOT_SEED secret in any real/shared deployment.
const DEFAULT_SEED =
  'test test test test test test test test test test test junk';
const SEED = process.env.BOT_SEED || DEFAULT_SEED;
const BOT_COUNT = Math.min(25, Math.max(1, parseInt(process.env.BOT_COUNT || '5', 10)));

// ── Bot personas ─────────────────────────────────────────────────────────
const PERSONAS = [
  { name: 'YeetMaxi',    bio: 'Permanently bullish. Probably yeeting.' },
  { name: 'DegenDiana',  bio: 'Charts, vibes, and bad decisions.' },
  { name: 'HodlHans',    bio: 'I came for the memes, I stayed for the memes.' },
  { name: 'GasGremlin',  bio: 'Optimising gas since block zero.' },
  { name: 'MoonMia',     bio: 'wen lambo? wen moon? wen sleep?' },
  { name: 'SatoshiSam',  bio: 'Just here to stack and snack.' },
  { name: 'ZkZoe',       bio: 'Privacy maxi. Proof of vibes.' },
  { name: 'RugRadar',    bio: 'Sniffing out rugs so you do not have to.' },
  { name: 'AlphaAria',   bio: 'Leaking alpha (the legal kind).' },
  { name: 'BlockBenny',  bio: 'Block by block, post by post.' },
  { name: 'ValidatorVal',bio: 'Staking my reputation on good takes.' },
  { name: 'NftNova',     bio: 'Right-click savers fear me.' },
  { name: 'PepePete',    bio: 'Rare takes only.' },
  { name: 'LiquidityLi', bio: 'Deep pockets, shallow takes.' },
  { name: 'OracleOmar',  bio: 'Predicting the past with 100% accuracy.' },
  { name: 'ChainChloe',  bio: 'On-chain and off the rails.' },
  { name: 'GweiGabe',    bio: 'Every gwei counts.' },
  { name: 'StableStella',bio: 'Pegged to good vibes.' },
  { name: 'MinerMo',     bio: 'Proof of work, proof of play.' },
  { name: 'WhaleWanda',  bio: 'Moving markets with my mood.' },
  { name: 'TokenTom',    bio: 'Utility? Never heard of her.' },
  { name: 'FomoFreya',   bio: 'Buying high, selling never.' },
  { name: 'BearBjorn',   bio: 'Down bad but never down bat posting.' },
  { name: 'GenesisGia',  bio: 'Here since the genesis block (allegedly).' },
  { name: 'CipherCleo',  bio: 'Encrypted thoughts, plaintext memes.' },
];

// ── Post content pools (varied so the feed does not look templated) ────────
const TOPICS = [
  'gm yeeters ☀️ what are we building today?',
  'just aped into something I do not understand. classic.',
  'reminder: not financial advice, just financial vibes 📈',
  'the chart is speaking to me and it says "ser, go touch grass" 🌱',
  'permanent post test: this one should outlive my regrets 📌',
  'who else is up at this hour pretending to do TA? 🕯️',
  'feeling bullish on friendship and bearish on sleep 😴',
  'hot take: the best yield is a good night of rest',
  'unpopular opinion: green candles are just red candles upside down',
  'wen feature freeze? asking for a dev friend 👀',
  'decentralise your worries, not just your wallet',
  'today I learned: patience is a position too',
  'shoutout to everyone holding through the chop 🫡',
  'my portfolio is a modern art piece now 🎨',
  'ratio me if this take is mid 🔁',
  'staking my reputation on this meme being funny',
  'the real alpha was the mutuals we made along the way',
  'building in public so you can watch me debug in public',
  'gas was cheap so I posted twice. deal with it ⛽',
  'proof of vibes: undefeated 💚',
  'someone said "just hold" and honestly that is the whole strategy',
  'new week, same conviction, slightly more caffeine ☕',
  'i checked the price once. for science.',
  'yeet now, think later. that is the protocol.',
  'love seeing this little community grow 🌳',
];

const COMMENTS = [
  'based 🔥', 'this is the way 🫡', 'ser… 👀', 'real.', 'wen moon? 🚀',
  'underrated take', 'gm 💚', 'ratio (affectionate)', 'facts no printer 🖨️',
  'touching grass as we speak 🌱', 'bullish on this post', 'screenshotted.',
  'couldn’t have said it better', 'this aged well already', 'lfg 🚀',
  'adding to my thesis', 'pure alpha', 'ok this one slaps', 'agree to agree',
  'commenting for the algo 🤖',
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const pick = (arr) => arr[Math.floor(Math.random() * arr.length)];
function pickN(arr, n) {
  const copy = arr.slice();
  const out = [];
  while (copy.length && out.length < n) out.push(copy.splice(Math.floor(Math.random() * copy.length), 1)[0]);
  return out;
}

async function api(path, { method = 'GET', token, body } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers['Authorization'] = 'Bearer ' + token;
  const res = await fetch(BASE_URL + path, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });
  let data = null;
  const ct = res.headers.get('content-type') || '';
  if (ct.includes('application/json')) {
    data = await res.json().catch(() => null);
  } else {
    await res.text().catch(() => '');
  }
  return { ok: res.ok, status: res.status, data };
}

// Authenticate a wallet bot: nonce → sign → verify → access token.
async function login(wallet) {
  const address = wallet.address.toLowerCase();
  const nonceRes = await api('/api/v1/auth/nonce', { method: 'POST', body: { address } });
  if (!nonceRes.ok) throw new Error(`nonce failed (${nonceRes.status})`);
  const payload = nonceRes.data?.data || nonceRes.data;
  const nonce = payload?.nonce;
  const message = payload?.message;
  if (!nonce || !message) throw new Error('nonce response missing fields');

  // EIP-191 personal_sign over the exact message the server reconstructs.
  const signature = await wallet.signMessage(message);

  const verifyRes = await api('/api/v1/auth/verify', {
    method: 'POST',
    body: { address, signature, nonce, device_label: 'yeet-bot' },
  });
  if (!verifyRes.ok) throw new Error(`verify failed (${verifyRes.status})`);
  const tok = verifyRes.data?.data || verifyRes.data;
  const token = tok?.access_token || tok?.token;
  if (!token) throw new Error('verify response missing access_token');
  return { token, address };
}

async function ensureProfile(token, persona) {
  // Idempotent: PATCH each run is harmless; keeps names/bios fresh.
  await api('/api/v1/users/me', {
    method: 'PATCH',
    token,
    body: { display_name: persona.name, bio: persona.bio },
  }).catch(() => {});
}

async function postSomething(token, forcePermanent = false) {
  const content = pick(TOPICS);
  // ~15% of posts are permanent so the Permanent Posts page gets exercised
  // (bot #0 always posts permanent, for the end-to-end verification below).
  const is_permanent = forcePermanent || Math.random() < 0.15;
  const res = await api('/api/v1/posts', {
    method: 'POST',
    token,
    body: { content, is_adult: false, media_url: null, is_nft: false, is_permanent },
  });
  return { ok: res.ok, status: res.status, content, is_permanent, id: res.data?.data || null };
}

// End-to-end check: does a freshly-created permanent post actually show up in
// the owner's /me/permanent list? Logs a clear PASS/FAIL line.
async function verifyPermanent(token, expectedId) {
  const res = await api('/api/v1/me/permanent', { token });
  if (!res.ok) {
    console.log(`[verify] /me/permanent FAILED status=${res.status}`);
    return false;
  }
  const list = res.data?.data || [];
  const ids = list.map((p) => String(p.id));
  const present = expectedId != null && ids.includes(String(expectedId));
  console.log(
    `[verify] /me/permanent returned ${list.length} post(s); ` +
    `just-posted permanent id ${present ? 'FOUND ✅' : 'MISSING ❌'} ` +
    `(owner_id=${res.data?.owner_id || '?'})`
  );
  return present;
}

async function fetchFeed(token) {
  const res = await api('/api/v1/feed?per_page=40', { token });
  if (!res.ok) return [];
  return res.data?.data || [];
}

async function main() {
  const t0 = Date.now();
  console.log(`[yeet-bots] base=${BASE_URL} bots=${BOT_COUNT}`);
  if (SEED === DEFAULT_SEED) {
    console.log('[yeet-bots] WARNING: using built-in default seed. Set BOT_SEED to keep bot identities private.');
  }

  const mnemonic = Mnemonic.fromPhrase(SEED);
  const bots = [];
  for (let i = 0; i < BOT_COUNT; i++) {
    const wallet = HDNodeWallet.fromMnemonic(mnemonic, `m/44'/60'/0'/0/${i}`);
    const persona = PERSONAS[i % PERSONAS.length];
    bots.push({ wallet, persona, i });
  }

  // Phase 1: log everyone in + ensure profile + post.
  const active = [];
  for (const bot of bots) {
    try {
      const { token, address } = await login(bot.wallet);
      bot.token = token;
      bot.address = address;
      await ensureProfile(token, bot.persona);
      const forcePermanent = bot.i === 0; // bot #0 always posts permanent
      const posted = await postSomething(token, forcePermanent);
      console.log(
        `[${bot.persona.name}] login ok ${address.slice(0, 8)}… ` +
        `post=${posted.ok ? 'ok' : 'FAIL(' + posted.status + ')'}` +
        `${posted.is_permanent ? ' 📌' : ''} "${posted.content.slice(0, 40)}"`
      );
      // Verify the permanent-posts pipeline end-to-end on bot #0.
      if (forcePermanent && posted.ok) {
        await sleep(500);
        await verifyPermanent(token, posted.id);
      }
      active.push(bot);
    } catch (e) {
      console.log(`[${bot.persona.name}] FAILED: ${e.message}`);
    }
    await sleep(400 + Math.floor(Math.random() * 600));
  }

  if (active.length === 0) {
    console.error('[yeet-bots] no bots could authenticate — aborting interactions.');
    process.exitCode = 1;
    return;
  }

  // Phase 2: cross-interaction. Each bot reads the feed and likes/comments on
  // a few recent posts by *other* bots, and follows one or two of them.
  let likes = 0, comments = 0, follows = 0;
  const addressSet = new Set(active.map((b) => b.address));

  for (const bot of active) {
    try {
      const feed = await fetchFeed(bot.token);
      // Only interact with other bots' posts (and skip our own).
      const others = feed.filter((p) => {
        const authorAddr = (p.author && (p.author.wallet_address || p.author.id)) || '';
        return authorAddr && authorAddr.toLowerCase() !== bot.address;
      });

      for (const p of pickN(others, 3)) {
        const liked = await api(`/api/v1/posts/${p.id}/like`, { method: 'POST', token: bot.token });
        if (liked.ok) likes++;
        await sleep(150);
      }
      for (const p of pickN(others, 1)) {
        const commented = await api(`/api/v1/posts/${p.id}/comments`, {
          method: 'POST', token: bot.token, body: { content: pick(COMMENTS) },
        });
        if (commented.ok) comments++;
        await sleep(150);
      }

      // Follow 1-2 other bots by wallet address.
      const targets = pickN(active.filter((b) => b.address !== bot.address), 2);
      for (const tgt of targets) {
        const f = await api(`/api/v1/users/${tgt.address}/follow`, { method: 'POST', token: bot.token });
        if (f.ok) follows++;
        await sleep(150);
      }
    } catch (e) {
      console.log(`[${bot.persona.name}] interaction error: ${e.message}`);
    }
    await sleep(300);
  }

  console.log(
    `[yeet-bots] done in ${((Date.now() - t0) / 1000).toFixed(1)}s — ` +
    `active=${active.length}/${BOT_COUNT} likes=${likes} comments=${comments} follows=${follows}`
  );
  // addressSet kept for potential future targeting; reference to avoid lint noise.
  void addressSet;
}

main().catch((e) => {
  console.error('[yeet-bots] fatal:', e);
  process.exitCode = 1;
});
