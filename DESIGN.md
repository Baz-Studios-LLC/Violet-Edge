# VIOLET EDGE — design reference

Living roster of hazards and enemies. **✅ implemented · 🔷 planned.** Behaviour notes are the
intended design; implemented rows describe what the code does today.

## Asteroids

| Colour | Status | Behaviour |
| --- | --- | --- |
| **Blue** | ✅ | The standard rock. Sizes L/M/S (radius 88 / 46 / 22); a hit splits it into two of the next size down, the smallest is destroyed. One bullet per break. |
| **Green** | ✅ | Dense — takes multiple hits (HP = size, so a large green needs 3). A normal bullet *chips* it; the chain beam, a mine blast, or a **mass shot** shear it in one. Introduced wave 6 (mixed with blue), waves 7–9 are all green. |
| **Orange** | 🔷 | Explosive. On destruction it detonates, destroying/damaging everything in a radius — other asteroids, mines, enemies (and the player). Detonating another orange in range sets off a **chain reaction**. |
| **Red** | 🔷 | Grows, like the Glutton boss: absorbs nearby asteroids to gain size. A large red that's broken splits into two, and *those* can absorb more and swell back up — an emergent "whack-a-mole" if you don't clear the field around them. |
| **Pulser** | 🔷 | Pulses bright white on a cycle; **invulnerable while lit**. You have to time shots to its dark phase. |

## Enemies

| Enemy | Status | Behaviour |
| --- | --- | --- |
| **Yellow mob** | ✅ | Standard enemy ship. Glides in, hovers and strafes around the player, steers clear of rocks/mines, lobs slow (dodgeable) shots, and flees off-screen after a lifetime. Runs in two windows: waves 3–4 and 8–9. |

### Bosses (every 5th wave; roster alternates on the 1–10 loop)

| Boss | Wave | Status | Behaviour |
| --- | --- | --- | --- |
| **The Warden** | 5 (·15·…) | ✅ | Roams the upper arena wearing a shield of captured asteroids on rotating arms. It grabs fresh rocks from the top half and hurls the smallest shield rock at you. Whittle the shield and shoot the core through the gaps — bullets damage the core; chain/warp do not. |
| **The Glutton** | 10 (·20·…) | ✅ | A red seeker that hunts free rocks and eats them to grow bigger (crowding you out) and tankier (healing). Starve it by clearing the field while you chip its HP with gunfire. Much higher HP than the Warden. |

## Related systems (see the port log in memory for detail)

- **Pickups:** chain shot (after the Warden, W5), mass shot (after the Glutton, W10), assist drone (W15, 🔷).
- **Achievements:** First Blood, Warden Off, Glutton for Punishment, True Blue (100 blue), Green Thumb (100 green), Edgelord (beat the arc), Purist (beat it with no powerups).
- **Wave arc:** 1–10, then loops. Bosses at 5 & 10. Content past 10 repeats 1–10 until the arc is dialled in.
- **Field population:** the on-screen count targets `POP_BASE + wave` (cap `POP_CAP`), topped up from the edges at `SPAWN_INTERVAL`. Edge spawns are ~80% large; a `BIG_FLOOR` keeps large rocks present even at the cap. Rocks that drift fully off-screen are recycled back in *only if large* — small debris usually despawns for good (mids sometimes), so breaking rocks apart can't silt the arena up with an overwhelming cloud of little ones; the top-up refills with fresh large rocks. The Warden grabs large/mid rocks for its shield and only resorts to a small one when nothing bigger is on-screen.
