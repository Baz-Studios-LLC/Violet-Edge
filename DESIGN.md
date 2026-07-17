# VIOLET EDGE — design reference

Living roster of hazards and enemies. **✅ implemented · 🔷 planned.** Behaviour notes are the
intended design; implemented rows describe what the code does today.

## Design rules

- **Asteroids are the theme.** Every other hazard exists to change *how you engage the rocks*, never
  to replace them. Enemies stay a fraction of the live asteroid count and thin themselves out — the
  field is always mostly rocks.
- **Each boss weaponizes one *relationship to asteroids*** (a verb): hoard · eat · shoot · prime ·
  pulse · pull · split · reflect · link. The boss is a lens on the rock field, not a separate spectacle.
- **Each powerup is thematically derived from the boss whose defeat drops it** — the reward echoes the
  mechanic you just beat (chain shot ← the Warden *linking* rocks; mass shot ← the red Glutton gaining
  *mass*; drone ← the enemy *ship*). The base Warp weapon is core kit and exempt.
- **Difficult but fair.** Manageable chaos is welcome; only pull back at unavoidable/instant death.
  Leave skill-gated tricks for players to discover rather than hand-holding them.

## Asteroids

| Colour | Status | Behaviour |
| --- | --- | --- |
| **Blue** | ✅ | The standard rock. Sizes L/M/S (radius 88 / 46 / 22); a hit splits it into two of the next size down, the smallest is destroyed. One bullet per break. |
| **Green** | ✅ | Dense — takes multiple hits (HP = size, so a large green needs 3). A normal bullet *chips* it; the chain beam, a mine blast, or a **mass shot** shear it in one. Introduced wave 6 (mixed with blue), waves 7–9 are all green. |
| **Orange** | ✅ | Explosive. Instead of splitting, a destroyed orange **detonates** after a brief fuse — a big AOE (`ORANGE_BLAST_R`) that **destroys everything inside outright** (rocks obliterated, not split), pops mines/enemies, kills the ship if caught, and lights other oranges (chain reaction). **Gold is spared**; bullet/chain/mine all *light* it. **Mechanic done; not yet in wave content** — dev **F3** spawns one to test. |
| **Red** | 🔷 | Grows, like the Glutton boss: absorbs nearby asteroids to gain size. A large red that's broken splits into two, and *those* can absorb more and swell back up — an emergent "whack-a-mole" if you don't clear the field around them. |
| **Pulser** | 🔷 | Pulses bright white on a cycle; **invulnerable while lit**. You have to time shots to its dark phase. |
| **Gold (1UP)** | ✅ | Not a hazard — a *reward*. A rare shimmering gold rock that drifts in at random times during play (any wave, boss waves included). Destroy the **whole lineage** (it + every fragment) for +1 life. Its pieces get a **long grace** (`GOLD_GRACE`, they recycle) so they're never lost *immediately* — but after that a piece that drifts off is culled and the life is **forfeit**, so clear them before they scatter. **Only your shots break it** — mines bounce off, the Devourer won't eat it. See Life economy below. |

## Enemies

Minor by design — capped as a fraction of the live asteroid count, and they flee/despawn over a
lifetime so the field stays mostly rocks. Three types total is the ceiling; more starts competing
with the asteroids for attention.

| Enemy | Status | Behaviour |
| --- | --- | --- |
| **Yellow mob** | ✅ | Standard enemy ship. Glides in, hovers and strafes around the player, steers clear of rocks/mines, lobs slow (dodgeable) shots, and flees off-screen after a lifetime. Runs in two windows: waves 3–4 and 8–9. |
| **Darter** | 🔷 | Fast interceptor — telegraphs, then charges in a straight line. Pure dodge, no ranged threat. **Chosen as the Act II mob (waves 12–13); next to build.** |
| **Miner** | 🔷 | Clings to and rides an asteroid as cover, popping out to fire. Kill the rock or catch it exposed. Deferred (Darter took the Act II slot) — a later act if we want a third type. |

## Bosses — the 50-level ladder

Every 5th wave. Each weaponizes a different *relationship to asteroids* (see Design rules). **Every
boss shows a top-center HP bar** (shared `boss_hp_bar`, tinted to the boss); the Glutton's tracks its
heal-toward-max, so letting it feed visibly refills the bar.

| Wave | Boss | Status | Verb → mechanic | Counterplay |
| --- | --- | --- | --- | --- |
| 5 | **The Warden** | ✅ | *Hoard* — shield of captured rocks on rotating arms; hurls the small ones | Strip the shield, shoot the core through the gaps (bullets hurt the core; chain/warp don't) |
| 10 | **The Glutton** | ✅ | *Eat* — red seeker devours free rocks to grow bigger & tankier; gorged to full it **OVERLOADS**: swells huge (flashing white as a tell), detonates a near screen-wide blast (wipes the field, kills you unless you're far — `DEVOURER_BURST_R`), then shrinks to nothing and starts over | Starve it (clear the field) + **shoot it down** — gunfire chips its HP *and* claws its size back (`DEVOURER_SHRINK_PER_HIT`), so active fire holds off the overload; when it's swollen and flashing, **get clear** before it bursts |
| 15 | **The Slinger** | 🔷 | *Shoot* — large enemy ship lines a big rock between you two and blasts it at you like a cannonball | Keep the lane clear / juke the shot; break its ammo first |
| 20 | **The Detonator** | 🔷 | *Prime* — turns nearby rocks into live bombs; itself armored | Bait the chain — only explosive blasts crack its shell |
| 25 | **The Pulsar** | 🔷 | *Pulse* — invulnerable while lit; shockwaves fling every rock (and you) outward | Hit only on the dark beat; don't get pinned to a wall |
| 30 | **The Singularity** | 🔷 | *Pull* — gravity drags all rocks + you into a crushing orbit | Thrust against the pull; let it crush on its own haul, or feed it an explosive |
| 35 | **The Hive** | 🔷 | *Split* — the boss **is** an asteroid; every hit mitoses, fragments re-fuse if ignored | Burn all pieces down before they merge |
| 40 | **The Prism** | 🔷 | *Reflect* — facets bounce your shots; spawns crystal rocks that also reflect | Catch an open facet, or shoot it *through* a rock |
| 45 | **Gemini** | 🔷 | *Link* — twin ships tethered by a shared rock core; damage transfers between them | Break the core rock to sever them, then focus one |
| 50 | **The Progenitor** | 🔷 | *All* — the first asteroid; cycles the earlier verbs as phases | Apply each phase's counter in turn — a full-run mastery check |

## Pickups (powerups) ↔ boss mapping

Each boss drops a powerup that echoes its own mechanic.

| Boss (drop) | Powerup | Status | Thematic tie |
| --- | --- | --- | --- |
| Warden (W5) | **Chain Shot** | ✅ | beam arcs/*links* between rocks — the Warden links rocks on its arms |
| Glutton (W10) | **Mass Shot** | ✅ | heavier, high-*mass* rounds — the red Glutton gains mass |
| Slinger (W15) | **Drone** | 🔷 | an ally *ship* that seeks & fires for you — mirrors the enemy ship |
| Detonator (W20) | **Warhead rounds** | 🔷 | your shots detonate & chain — echoes the primed bombs |
| Pulsar (W25) | **Nova pulse** | 🔷 | a shockwave that shoves rocks away — echoes its pulse |
| Singularity (W30) | **Magnet** | 🔷 | pulls pickups/small rocks in — echoes its gravity (base Warp stays core kit) |
| Hive (W35) | **Spread shot** | 🔷 | your shot *splits* into several — echoes mitosis |
| Prism (W40) | **Ricochet rounds** | 🔷 | bullets *reflect* off walls/rocks — echoes the facets |
| Gemini (W45) | **Twin cannons** | 🔷 | two linked fire streams — echoes the twins (kept distinct from the drone) |
| Progenitor (W50) | — | — | final boss; no drop (or a combined ultimate) |

> Resolve at implementation: Drone (W15) vs Twin cannons (W45) must feel distinct, and Magnet (W30)
> must not just re-skin the base Warp weapon.

## Progression — 5 acts

Each new asteroid debuts a few waves *before* the boss that weaponizes it: learn the toy, then fight
the thing made of it. (This replaces the current "loop 1–10" scaffold; until the arc is built,
content still repeats 1–10.)

| Act | Waves | New asteroid(s) | New enemy | Bosses |
| --- | --- | --- | --- | --- |
| I — The Field | 1–10 | Blue, Green ✅ | Yellow mob ✅ | Warden (5), Glutton (10) |
| II — Volatile | 11–20 | Orange (explosive) ✅ | Darter | Slinger (15), Detonator (20) |
| III — Unstable | 21–30 | Red (growing), Pulser (invuln-lit) | — | Pulsar (25), Singularity (30) |
| IV — Deep Belt | 31–40 | **Crystal** (reflects) *or* **Ice** (shard-burst) — TBD | — | Hive (35), Prism (40) |
| V — The Core | 41–50 | **Void** (swallows bullets) *or* **Magnetic** (bends fire) — TBD | — | Gemini (45), Progenitor (50) |

### Waves 11–15 — building now (won't extend past 15 until a no-dev-invuln run reaches 16)

Per-wave content plan (the current 1–10 loop stops applying from 11):

| Wave | Content | Section status |
| --- | --- | --- |
| 11 | green + orange | orange ✅ · wiring pending |
| 12 | Darter (new mob) + orange | mob + wiring pending |
| 13 | green + orange + Darters (as 12) | mob + wiring pending |
| 14 | orange only | wiring pending |
| 15 | **The Slinger** (boss) + green only | boss + wiring pending |

Build order (one section at a time): **1. orange ✅ → 2. Darter mob → 3. Slinger boss → 4. wire the
11–15 content.**

## Life economy (implemented: gold 1UP rock)

50 levels on 3 lives is likely impossible, especially a no-powerup **Purist** run — so lives are
recoverable, but only by earning them, via a rare gold asteroid. ✅ Implemented:

- A gold rock **drifts in at a randomized time during play** (a countdown, not tied to wave starts) —
  a distinct shimmering gold large rock that otherwise behaves normally (splits when shot). One hunt
  at a time; a long random gap measured from when it *appears* (`GOLD_MIN_GAP`..`GOLD_MAX_GAP`, ~4-6
  min) is armed on each spawn, so at most ~1 appears per (3-min) wave and never back-to-back.
  `GOLD_INITIAL_DELAY` graces the run start. Any wave, boss waves included (the Devourer won't eat it;
  a rock the Warden grabs is just shot off its shield).
- You must **destroy the whole gold lineage** — the rock *and* every gold fragment (gold-ness is
  inherited through every break: bullet, chain, mine) — to claim **+1 life**. `GoldRush` tracks it.
- **Long grace, then forfeit.** Gold fragments carry a long grace (`GOLD_GRACE`) during which they
  recycle rather than being culled — so a shot gold never vanishes *immediately*, you get a fair
  window to catch every piece. After the grace, a piece that drifts off-screen IS culled and latches
  `forfeited` (the life is denied even if you clear the rest). So gold can be lost — just not instantly.
- Capped at `LIFE_CAP` (= `START_LIVES`, 3): a gold rock only restores a *lost* life, never above the
  starting count. Purist-safe: a life isn't a powerup.
- **Telegraph:** a single shimmering gold outline (same shape/chunkiness as any rock — the pulsing
  colour is what marks it); clearing it pops an "EXTRA LIFE" toast + a distinct 1UP jingle
  (`life_sfx_wav`, separate from the achievement chime).
- **Only player shots break it.** Mine blasts spare gold rocks, and a drifting mine bounces off one
  instead of detonating — so a mine can't clear the lineage for you. The **Devourer** won't eat gold
  either (both would hand over a 1UP the player didn't earn).

Considered and shelved (could layer on later): score extends, boss-clear +1, perfect-wave meter.

## Related systems

- **Scoring** (classic-Asteroids values — smaller rock = more points, so *finishing* a rock beats cracking it):

  | Target | Points |
  | --- | --- |
  | Asteroid — large / mid / small | 20 / 50 / 100 |
  | Green (dense) | ×2 (40 / 100 / 200) |
  | Enemy mob | 300 |
  | Mine | 150 |
  | Boss | 3000 |
  | Warden shield rock (small remnant) | 20 |
  | Rock swallowed by the warp | `WARP_ROCK_SCORE` (25, low flat — no farming) |

  Gold rocks score like normal rocks (their reward is the life, not points). Score is purely for
  ranking — it doesn't grant lives.
- **High scores:** a persisted **top 5** (numeric), saved to `violet-edge.hiscore`. On game over the
  final score slots into the table (`record_high_score`), the screen shows the board with the new
  entry lit and a **NEW BEST!** / **TOP 5!** banner, and the main menu shows a **BEST** line.
- **Achievements:** First Blood, Warden Off, Glutton for Punishment, True Blue (100 blue), Green Thumb (100 green), Edgelord (beat the arc), Purist (beat it with no powerups). New bosses each get one, named for the boss.
- **Field population:** the on-screen count targets `POP_BASE + wave` (cap `POP_CAP`), topped up from the edges at `SPAWN_INTERVAL`. Edge spawns are ~80% large; a `BIG_FLOOR` keeps large rocks present even at the cap. Rocks that drift fully off-screen are recycled back in *only if large* — small debris usually despawns for good (mids sometimes), so breaking rocks apart can't silt the arena up with an overwhelming cloud of little ones; the top-up refills with fresh large rocks. Freshly-broken fragments get a short grace window (`FRAGMENT_GRACE`) during which they always recycle rather than being culled, so a rock shattered right at the edge can't lose its pieces off-screen before you get a shot. The Warden grabs large/mid rocks for its shield and only resorts to a small one when nothing bigger is on-screen.
