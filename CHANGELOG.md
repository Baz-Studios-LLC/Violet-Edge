# VIOLET EDGE — Changelog

Patch notes for the Rust + Bevy build. Newest first. (Releases are cut to GitHub and picked up by the
Baz Studios launcher.) **Keep this current with every change** — it's the record testers read.

## v0.5.1 — The Gallery, the NG+ bestiary, and a kinder 1UP (2026-07-31)

- **AEGIS SHARDS — a NEW GAME+ exclusive powerup.** Beat the Warden+ on lap two and it gives up its
  own trick: three small shards ride a slow orbit around your hull, moving with the ship, and each
  one **grinds a rock that would have killed you** — vaporized, no chunks. It is deliberately *not*
  invincibility: every save spends a shard, they regrow **one at a time on an 11s cooldown**, and
  with the ring empty the next rock kills you as usual. The thinning ring is its own readout, so
  there's no HUD slot to read. It replaces the Chain orb on lap two rather than joining it — one orb
  per boss keeps the calm readable, and NG+ has no beacons past wave 5 for the beam to answer.
- **THE HUNTER — a new asteroid, debuting wave 6.** The first rock that comes *after you*: it steers
  at your ship and gets hungrier the longer it lives, so you can't park and farm. It's always
  outrunnable (capped well under your top speed) and breaking one **resets the hunt** — the chunks
  inherit it but start docile, so a split really does buy breathing room. Wave 6 is its teaching
  wave, it garnishes 7-9, and it retires with Act I at wave 10. It joins the wave-30 all-types
  finale, and has its own achievement (**Who's the Prey Now** — 350 hunters).
- **THE LAPSE — a rock that isn't always there** (NG+ roster). It fades out **completely** — nothing
  is left where it was, not even a hint — then slowly materializes **somewhere else on the field**
  when it's about to become dangerous again, on a randomized clock. The whole return is a **free
  warning**: while it's gone *and* the entire time it's fading back in, it can't hit you and you
  can't hit it, the fade-in is long enough to read and fly out of (compile-time asserted, so it can
  never be tuned into a cheap death), and it never materializes within 170px of your ship. Its
  chunks keep phasing on their own clocks.
- **THE TENDER — the Belt's repair crew** (NG+, late waves). It doesn't shoot. It finds two of the
  fragments you left behind, takes hold of both with tractor beams, and **fuses them back into a
  whole rock** — a split run backwards. Shoot the drone or either fragment and the weld fails; leave
  it alone and the field stops shrinking. One hit kills it, one at a time on the field, and it never
  touches you directly: it's the first mob that's a genuine priority target rather than a nuisance.
- **NEW GAME+ runs its own bestiary.** Waves 1–5 are a **pure recap** — the old roster only, so the
  opener is familiar ground. From wave 6 the old roster **retires entirely** and lap two runs new
  rock types instead (the Hunter and the Lapse so far; each one added widens the mix). Density eased
  to match: **+6 rocks per wave → +3**, because with homing, phasing and *reconstituting* pressure in
  the roster the rock types carry lap two's difficulty and raw volume doesn't have to.
- **Life rocks are far less punishing.** The 1UP hunt no longer produces small fragments at all: a
  gold rock sheds two mid-sized pieces and those break clean, so it's three solid targets instead of
  seven with four tiny stragglers. Those stragglers were the whole problem — a small piece that
  crosses the edge after its grace is gone for good most of the time, and it took the life with it.
  Mid pieces wander off far less often, and the grace window is unchanged.
- **THE GALLERY — a bestiary in the menu.** Every rock, hazard and boss gets its **own page** (20 of
  them), drawn large as real vector art built the same way the field versions are, with a name, a
  role line, and a **field report in the pilot's own voice** — because the lore of this game lives in
  its menus, every page carries a piece of the mystery (rocks assaying as mantle stone and core iron,
  machined mines left on your route, a belt that keeps time, something that *maintains* it) alongside
  what you actually have to do about the thing. **The book GROWS**: a page is added when its subject
  is first introduced to your field, and there are no blank or locked pages to leaf through — so the
  gallery can never spoil what's still out there. Page with A/D or the arrows. (Existing saves start
  the book empty and fill it in as you play.)
  - **Every page's art is scaled to fit.** Wide entries — the Beacon's aura, an orange's blast reach,
    the Warden's penned rocks — were drawing past their band and printing through the name and
    description. Each page now scales to a hard radius budget based on its own true extent.
  - **The art matches the game.** The mine was drawn as a spiked ball when the real one is a crimson
    diamond; the raider was an arrow when it's a throbbing orb; the well had three arms instead of
    seven. All three are now built exactly the way their field versions are, so the reference can't
    lie about the game.
  - **No more black gashes through the pictures.** Off-run the grid's colour dims to pure black, but
    the lines were still being stroked — and since the world render runs after the gallery's art
    layer, those black strokes painted straight over the artwork. The grid is now skipped entirely
    when no run is active.
  - One-per-page also settles the palette question: several entities share a hue because the neon
    spectrum is full, and on separate pages they're never compared side by side.
- **Menu breathes again.** Seven buttons had it packed shoulder to shoulder: the masthead and
  wordmark are trimmed a little and the buttons now live in their own column with a wider gap, so
  the screen reads open instead of stacked. (The overlay's shared tight spacing is untouched — the
  dense screens like Controls and Pilot Log depend on it to fit.)
- **The dev god-mode ring can't be mistaken for gameplay.** It was a solid circle sitting at almost
  exactly the Aegis shards' orbit radius, so with the shards up it read as a rail joining them. It's
  now a broken reticle of rotating arcs further out — still unmissable, since god-mode should never
  look subtle. (Release builds can't show it at all.)

## v0.5.0 — A real soundtrack (2026-07-30)

- **The whole soundtrack is produced music now.** The main and boss themes join the game-over
  theme as Antigravity-generated tracks (embedded, so the exe stays self-contained): the main is a
  high-energy arcade drive with detuned supersaw leads, acid sub-bass and 909 drums; the boss is
  dark industrial with tritone stabs and distorted sub-rumble. Both loop gap-free and are
  level-matched to the rest of the mix — they arrived mastered hot (peaking at full scale, 4.3 dB
  and 2.7 dB above the old score), so each carries a measured trim that also keeps headroom for
  the sound effects.
- **The post-boss corruption is paused, not lost.** The main ships as a single track for now, so
  the tier system stays wired but dormant (the tier index clamps to the tracks that exist — which
  also guarantees the music can never restart mid-run at an act boundary). Drop in per-act variants
  later and it wakes up untouched.
- **Retired code deleted, not parked.** The procedural music score (~280 lines: the club
  arrangement, the six-tier corruption DSP, and its instrument voices) is gone now that produced
  tracks have replaced it — `git log` keeps it if it's ever wanted back. All the sound effects and
  the boss buildup riser are still synthesized as before.

## v0.4.9 — The first produced track (2026-07-30)

- **The Game Over theme is now a produced track.** Generated in Antigravity from the procedural
  dirge as reference: melancholic ambient synthwave — warm analog pads over Am–Fmaj7–Dm7–E7,
  Rhodes-toned arpeggios, soft sub bass (15s, loops gap-free). Embedded like every other asset, so
  the exe stays self-contained, and level-matched to the rest of the score (the produced master
  runs ~1.6 dB quieter, so it carries a measured gain trim). The procedural bell dirge is retired
  to reference/fallback duty — this is the first externally-produced track in the score.

## v0.4.8 — NEW GAME+, the split economy, and the big balance pass (2026-07-30)

- **The split economy** (user design): breaking a rock no longer guarantees two children. A large
  sheds **1 or 2 mediums**; a medium either shatters into **2 smalls or dies outright**. A large's
  lineage averages ~4.4 rocks instead of always 7 — small debris stops crowding the screen, and no
  break is perfectly predictable anymore. Gold lineages and red rocks keep their guaranteed splits
  (the 1UP hunt's length and red's regrow identity are tuned around them); clusters still shatter
  their own way.
- **THE BALANCE PASS — bosses earn their banners, runs fit an evening.**
  - **Boss HP roughly doubled across the ramp** (26/34/40/46/52 → 50/60/85/72/90): they were
    burning down too fast. Scaled per fight, not flatly — the Slinger (exposed core all fight)
    takes the biggest bump; the Detonator (hittable only in priming windows) the smallest. **The
    Phantom's phases top the whole ramp**: 95 HP per phase (285 total, was 3×30) — no phase of the
    finale is ever a softer fight than any boss before it. NG+'s ×1.5 stacks on top.
  - **The Warhead is a siege weapon now**: still toggled on the Q-cycle, but the rate of fire is
    VERY slow (1.3s between rounds, was 0.28). Each round clears a 110px disk — so now it's aim,
    fire, wait, not a screen-clearing machine gun.
  - **Arcade run length**: wave timers cut 120s → 100s. A full clear lands around ~50 minutes
    with the heavier bosses; a wave-25 death costs less of an evening. The gold 1UP cadence is
    rescaled to match (first gold at 40s), so lives-per-wave stays the original tuning.
- **The warp hole has a voice** (v2 — the first cut's pulsing churn read as "a dog sniffing," so
  the pulse is GONE). One continuous inhale now: a filtered roar sweeping deeper as the hole
  feeds, a falling air whistle, a low drone — collapsing into the deep thump as it snaps shut
  (rendered to the hole's 2.6s life; the launch whoosh is unchanged and layers over it).
- **Small rocks trimmed** (radius 30 → 26). The hittability bump overshot — the smallest debris was
  eating too much screen. 26 keeps them an easy target while reading as debris again. (Balance
  audit follow-ups tracked separately.)
- **NEW GAME+.** Beat the game once (ever) and a `NEW GAME+` button appears on the menu — forever.
  It's essentially a new game: same fresh start, nothing carried over, every achievement still
  counts. What changes is the Belt: a denser field from wave 1 (+6 rocks over the whole curve,
  finale included; mobs and mines scale with it), boss cores half again as tough, and the music
  already corrupted a tier when you arrive. The HUD wave line carries a quiet `NG+` tag, restarts
  stay in the mode, and a normal PLAY clears it. Button-only by design — the second lap is chosen,
  never stumbled into. And because the lap assumes mastery:
  - **Waves 1-5 roll the full rock roster** — every type the Belt has, from the first wave. The
    teaching rosters are for the first game; act arcs resume from wave 6.
  - **Every boss carries the mark** — the warning banner and HUD read `THE WARDEN+`, `THE
    GLUTTON+`, and so on.
  - **THE WARDEN+ fights like it means it**: throws come at 0.65× cadence as a TWO-rock spread,
    and every hurled rock is a PRIMED live bomb on a 1.7s fuse — shoot it out of the air or clear
    the blast radius. (Upgraded mechanics for the other five bosses arrive one at a time.)
- **The Phantom's planned drop is cut.** The wave-30 kill rolls straight into the ending — there
  was never anything to pick a drop up with, so the finale boss now *officially* drops nothing.
  Its reward is the ending itself, and the NEW GAME+ unlock.

## v0.4.7 — The juice pass, a corrupting score, and the studio splash (2026-07-30)

- **THE JUICE PASS — kills land now.** The feel layer the game was missing, all photosensitivity-safe
  (freezes and smooth motion, never strobes):
  - **Hit-stop**: the world freezes for a breath on the big moments — your death (0.12s), a boss
    core detonating (0.14s), the Nova Shield eating a lethal hit (0.07s). Capped, never stacking.
  - **Screenshake**: trauma-based, smooth layered sines (no jitter), a few px at most. Deaths and
    boss kills slam; mine blasts rattle; a STREAK of big rocks stacks into a visible rumble while
    single pops stay calm. Driven off the existing sound events — anything that sounds big feels big.
  - **Kill pops**: every size-2+ rock death leaves a fast type-colored ring on every kill path
    (bullet, chain, mine, warhead, blast). Small rocks skip it so the field never washes out.
  - **Bosses finally SOUND like they die**: a deep falling-sub detonation boom on every boss kill
    (they used to go down with particles only).
- **The music corrupts as you descend — same song, playing WRONG.** The composition never changes;
  how it plays back does. Each boss down, the whole track sags like a dying tape — slower and
  flatter every act (128 BPM in tune → 112 BPM two semitones flat by the last) — and the melody's
  instrument is swapped colder while playing the same notes: supersaw → hollow square → acid
  squelch → FM metal. The wrongness layers pile on with it: ghost tritone drone + pitch wobble
  (boss 1+), industrial clangs (2+), the boss track's reese growl in the drops (3+), sour ♭9 stabs
  (4+), wrong notes and static bursts (final act) — with DSP grit (drive/bitcrush/decimation)
  deepening on top. Menu and post-win screens play the clean mix — beating the Phantom literally
  hands the music back. (New dev tool: `cargo test render_tier_previews -- --ignored` renders 24s
  previews of every tier for audition by ear.)
- **The Warhead detonates on impact now.** The round no longer pierces on forever — it spends
  itself on the first rock it hits, and the violet ring finally means something: everything inside
  the blast radius dies with it (credited to you; gold is spared, lit pulsers shrug it off, and
  beacon auras do NOT protect against it — blasts are their designed counter). One aimed round
  into a packed lane is now a real answer, and it visibly ENDS there.
- **Game Over has its own music.** A tolling two-note bell knell (inharmonic partials, long
  decays, one strike every 2.4s) over a breathing sub drone and a whisper of cold wind — no drums,
  no saws, nothing shared with the rest of the score. A run ending now *sounds* like its own world
  (it used to be silence). Synthesized like every other track.
- **Light ribbon extended again** (46 → 72 points, ~1.2s of motion) — the Tron trail reads as a
  real presence behind the ship now.
- **Baz Studios boot splash.** The game now opens on the studio card — black screen, the BAZ
  STUDIOS logo fading in with the logo sting (the same logo + sound Wingman ships, embedded so the
  exe stays self-contained). Auto-dismisses into the menu at ~3.5s; any key/click/pad press skips
  ahead to the fade-out (never a hard cut). Music holds silent until the menu so the sting owns
  the boot moment.

## v0.4.6 — Cleaner flight, a sealed log, and the Pacifist run (2026-07-29)

- **Mine blasts score nothing — the mine itself still pays.** Destroying a mine (shot, chain beam,
  warp, blast chain) awards its 150 as before, but the rocks its explosion shatters are now worth
  ZERO — points come from aimed play, not from standing near a blast, so mine chains can't be
  farmed for score. (Orange and Warhead blasts still score their rocks: the player lit those.)
- **Drift tightened** (playtest: "the drift is too great"): drag up (`FRICTION` 0.15 → 0.10) with
  thrust raised in lockstep (1000 → 1200) so top speed stays ~520 — the glide-out after releasing
  thrust is ~20% shorter without the ship getting any slower. Still a drift game, deliberately.
- **PACIFIST achievement** (#24): clear two straight waves breaking *nothing* — no rocks, mines,
  enemies, or golds, no warp fired, and no powerup fired (chain beam, mass round, warhead round all
  count, hit or miss). Dying is fine — the test is restraint, not survival. Tracked as a per-wave
  delta of the kill counters plus a powerup-fire counter (one watcher, zero flags sprinkled through
  kill sites); boss waves always break the streak, and it never spans a restart.
- **Flight-feel pass** (hitbox untouched — this is handling, not forgiveness):
  - **Turn rate 4.6 → 5.2 rad/s** (~300°/s): a 180° flip lands in ~0.6s, so weaving a gap answers
    your hands instead of arriving late; taps still resolve ~5° for fine aim.
  - **Analog trigger thrust on controller** — the RT bind now reads the trigger's actual pull, so a
    half-pull is a half burn. Feathered approaches are real now; face-button/keyboard thrust still
    reads full-on.
  - **Stick deadzone smoothed** — turn input past the deadzone rescales from zero instead of jumping
    to 0.2, killing the kink when easing into a bank.
- **The Pilot Log starts sealed.** THE BELT no longer reads before you've ever flown — it decrypts
  the moment your first run launches (the locked slot says "Awaiting deployment."). A brand-new
  player now opens an all-static log: the story only exists once the pilot is out there.
- **PILOT LOG UPDATED toasts.** When an entry decrypts — first launch, or a boss's record as it
  falls — a toast pops in the top-center column (titled in that entry's accent color) with a dry
  two-blip radio cue, distinct from the achievement chime. Old saves never re-toast on boot, and
  the toast card itself is now one shared helper across achievements / extra-life / log events.

## v0.4.5 — 23 achievements, progress on every death, and a Detonator that fights (2026-07-29)

- **Every death now shows your progress.** The Game Over screen gained two lines under the score:
  - `REACHED WAVE 23 — BEST 27` — a persisted **best-wave record** (gold `NEW BEST!` when you just
    set it). Getting deeper is now a visible, bragging-rights stat, not a feeling.
  - A **nearest-achievement ticker** (e.g. `TRUE BLUE  612 / 1000`) — the lifetime grind closest to
    unlocking, because every run advances something even when it ends on a rock.
  No difficulty was touched and nothing carries between runs — the game stays a one-credit arcade
  run; what persists is your record, not your power.
- **The Detonator fight flows now.** Two fixes for the same stall — the boss spending long stretches
  armored, "looking for a green":
  - Its priming channel (the vulnerable window) runs **2.5s, up from 1.5s** — long enough to land
    real damage once you've closed the distance.
  - **No orange rocks spawn on wave 20 anymore.** The boss can't prime an explosive (it's already a
    bomb), so every orange was a dead slot it had to drift past. Its wave is pure green fodder — the
    Detonator itself brings the explosions.
- **Warhead rounds look ARMED now.** They used to draw exactly like standard shots; a warhead round
  is now a violet dart-shell with a slow-spinning ring of detonation ticks (the same visual language
  as its HUD glyph and blast ring) — instantly distinct from the standard orb and the fat mass round.
- **Beacon aura enlarged** (200 → 270 radius, nearly double the area) — the shield zone now genuinely
  owns a region of the field instead of hugging the rock. The reach ring is drawn a touch brighter so
  the boundary reads at a glance. Mechanics audited + newly test-pinned: warhead rounds fizzle inside
  the aura like everything else, rocks past the edge break normally, the beacon itself is always
  shootable, and blasts/warp/red-absorb still bypass it (the counterplay).
- **Longer light ribbon.** The ship's Tron trail carries ~3/4s of motion now (was ~1/2s).
- **Pilot Log fits the screen.** The 8-entry journal was overflowing the frame (title and BACK
  clipped); the reports now stack in a tight column and the whole screen sits comfortably inside.
- **23 ACHIEVEMENTS** (up from 12) — built for a game you restart a lot. All lifetime, all persistent:
  - **One per rock type**: True Blue (1,000 blues), Green Thumb (500 greens), Demolition Derby
    (400 oranges lit), Beat It (300 pulsers on the dark beat), Seeing Red (400 reds), Ice Breaker
    (300 clusters), Keymaster (100 beacons).
  - **The restart ladder**: Back for More (10 runs), Sisyphus (25), The Definition of Insanity (50) —
    every press of Start counts, and the tally saves immediately.
  - **Lifetime grinds**: Wave Goodbye (250 waves cleared — boss kills count), Event Horizon (150 warp
    holes opened).
  - **Untouchable** — beat the game without losing a single life (a Nova Shield absorb doesn't
    count against you; a real death does).
  - The achievements screen packs all 23 rows cleanly into the frame.
  - Save format extended (old saves load fine — new counters just start at zero).
- **The Glutton has real fangs now.** Its teeth were reading as thin "V" chevrons; each tooth is now a
  closed fang with a bright center rib (solid under bloom), chunkier and fewer per ring.
- **Menus fit the screen.** The Controls screen was clipping top and bottom; every menu got a layout
  pass — tighter row spacing, slimmer buttons, wider rebind slots so bindings never wrap — and the busy
  screens now sit comfortably inside the frame at any window size.
- **Briefing objective rewritten**: "Investigate and hold back the approaching mass." (The old
  sign-off line is gone.)

## v0.4.4 — Living bosses, the act-owned belt, and two new rocks (2026-07-28)

- **EVERY BOSS IS A SPECTACLE NOW.** All six got full visual redesigns — and none of them ever sits
  still:
  - **The Warden** is an armored vault: counter-rotating octagon shells around an eye that TRACKS you,
    with segmented tentacles that ripple and writhe as its shield array spins — idle arms wave, and a
    capture visibly REACHES. Dying, the tentacles shear off one by one before the shell goes.
  - **The Glutton** is a living maw: two counter-rotating rings of gnashing teeth around a gullet that
    glows brighter the more it gorges, feeler-spines waving, and it LUNGES at prey in surging bites.
    It dies in three deflating spasms.
  - **The Slinger** is a proper railgun gunship: twin rail prongs frame the charging round, a slotted
    ammo drum spins behind the cockpit, engine pods burn — and the whole hull KICKS BACK on every
    launch. Dying, it lists off its heading while wings shear and pods pop.
  - **The Detonator** is an armored bloom: six petal plates that HINGE OPEN when it primes — the
    vulnerability window is literally visible — around a caged core, its priming beam now MARCHING
    dashes. The petals blow off one by one as it dies, baring the failing core.
  - **The Pulsar** is a living star: eight spikes that extend blazing toward the lit beat and retract
    to a dim skeleton when it's dark (the extension telegraphs the invulnerable window), wrapped in
    counter-rotating gyro arcs, swaying on a slow orbit. It dies shedding spikes, then goes NOVA.
  - **The Phantom** keeps its skull — and gains tattered cloak wisps that sway beneath the jaw.
- **A real WARNING banner.** The boss run-up now frames its warning in a hazard band with marching
  edge-ticks and the boss's TRUE body (mini, alive, the same drawing the fight uses) beside its name.
  The background cameo uses the same canonical bodies — what you're warned about is what arrives.

- **The Limpet is gone.** The rock-riding parasite mob (waves 12–13) is removed outright — the
  asteroids are the star, and waves 12–13 are pure rock waves now.

- **Each act owns its asteroids now.** No rock type outlives its act: Act I is blue (green bridges in
  late), Act II runs green + orange + pulser and retires all three at wave 20, and **Act III belongs
  entirely to its own types** — red as the carrier (wave 21 is the all-red teaching wave, and the Pulsar
  is fought over a pure red field), joined by the beacon and the cluster. The wave-30 finale stays the
  one exception, rolling every type at random.
- **TWO NEW ASTEROID TYPES fill out Act III** (waves 21–29 ran the same three-rock recipe for 8 waves):
  - **The BEACON (wave 23+)** — a teal aura warden. Every rock inside its aura is **immune to your guns
    and the chain until the beacon falls**: the field becomes a question of what you shoot *first*.
    Blasts, the warp, and hungry reds ignore the aura — those are your answers. Rare, dense, never splits.
  - **The CLUSTER (wave 26+)** — fractured pale ice, visibly cracked. Breaking it **shatters it into a
    ring of fast shards** instead of splitting in two — stop shooting things at point-blank. The mass
    shot vaporizes it clean and the warp swallows it whole: the first rock where your tool choice
    really matters.
- **The wave-30 finale field is fully random now.** Instead of mono-color groups, the Phantom's arena
  trickles in **every rock type the Belt has shown, rolled at random** — same gentle rate, small field
  cap, so it's variety, never a wall.
- **The finale boss is THE PHANTOM, everywhere.** Every remaining player-facing "Haunt" (achievement text,
  the Pilot Log records) now says the Phantom, matching its boss-warning name.
- **Each Phantom phase ends like a real boss kill.** Depleting a phase now fires the same big double blast
  every other boss dies with, and the spent form crackles apart before what's left reforms into the next
  shape. The final death drops the pulsating light show: it crackles apart while being drawn to center,
  erupts in ONE grand screen-filling blast, then keeps a steady (never rhythmic) dying crackle going while
  the scene plays out. The ending text no longer mentions what flees east — that's for sharp eyes only.
- **The Nova Shield is your own silhouette now.** The shield shell is the ship's shape scaled outward — a
  second hull layer that turns with you — instead of a floating hexagon. The HUD SHIELD icon matches
  (a little ghost-ship outline).
- **The Detonator no longer primes orange rocks** — they're already bombs; charging one was redundant. Its
  munitions are its own red-white primed rocks, cooked from plain asteroids only.
- **MODE slot and mode name are one thing.** The equipped-shot name (STANDARD / MASS / WARHEAD) now sits
  right beside the MODE slot's glyph on the ability strip, flaring on a Q toggle — no more separate
  floating label.

## v0.4.3 — Nova Shield, achievements, the Pilot Log, and a real HUD (2026-07-28)

- **Pause now shows your controls.** The pause menu carries a read-only controls card — every action with
  its current binding, for whichever device you're actually using — so checking a key never costs a run.
  (Rebinding stays on the main menu's CONTROLS screen.)
- **NEW POWERUP: the Nova Shield** — the Pulsar (wave 25) now drops its reward orb. A regenerating
  **one-hit barrier**: while up it eats one lethal hit — any hit: rock, mine, boss, beam — and collapses;
  after ~9 seconds it **flickers back online**. Get hit while it's down and that's a life, as normal. The
  player inherits the Pulsar's own lit-invulnerable ↔ dark-vulnerable rhythm: a glassy violet ring shows
  it's up, blinks as it re-lights, with its own shatter / re-light sounds.
- **Bolder post-warp crackle.** The electric flicker the grid does around a warp is much more pronounced
  (amplitude only — the crackle rate is unchanged and stays photosensitivity-safe).
- **Achievements overhauled — 7 → 12, and "beat the game" is finally honest.** *Edgelord* used to unlock at
  boss 2 (the old 10-wave arc); it now requires the real thing: **defeating the Haunt at wave 30** — and
  *Purist* likewise now demands the full no-powerup run. New achievements: **Outgunned / Defused / Lights
  Out** (bosses 3–5), **Minesweeper** (250 mines), and **Gold Rush** (25 extra lives earned). Lifetime
  grind targets raised across the board (1,000 blue / 500 green) — you'll die a lot; these are careers,
  not errands. Old save files carry over cleanly.
- **Actual lore — the PILOT LOG.** The game has a story now, and it stays a mystery you assemble. A new
  **PILOT LOG** screen on the main menu holds the field reports your pilot transmits home — one per
  contact, **decrypting the first time each boss falls**. The early reports only observe (a field holding
  formation… a thing *penning* rocks?); the truth accumulates in details — wrong minerals in the debris,
  strata inside a cracked rock, a plotted heading — and only the **final entry** says it plainly: what the
  Belt really is, what sent the Haunt, and why the story isn't over. The BRIEFING opens cold and urgent
  (a mass approaching fast; the prototype VIOLET CUTTER deployed — one pilot, possibly the only chance).
  The ending scene itself is unchanged — the core's escape was always the sequel.
- **A Tron-style light ribbon replaces the exhaust sparks — and the ship is solid neon now.** The spark
  particles that used to trail behind the ship as broken dashes are gone. In their place: a **fading light
  ribbon** in the ship's own violet, streaming from the exhaust — the light-cycle wall, minus the
  lethality (~half a second long, tapering to a point, purely cosmetic). The thrust flame stays exactly as
  it was, the hull keeps its classic dart shape, and it's now **filled in the ship's neon purple — a
  bright rim over a slightly darker core** — the one filled body in a wireframe world, so the player
  always pops. The finale send-off ship and the HUD lives icons get the same filled look.
- **A proper HUD, all along the top — and every light is named.** The warp and chain charge pips move up
  from the bottom into a **labeled ability strip** under the score: each slot carries its name — **WARP ·
  CHAIN · MODE · SHIELD · DRONE** — and **appears as you earn it** (a fresh run shows just WARP). CHAIN
  keeps its pips + recharge bar; **MODE always shows what Q has equipped** (standard round / fat mass
  round / ticked Warhead); **SHIELD** shows the Nova's state (bright hex up, regen bar while it rebuilds);
  DRONE shows your wingman. The Q-toggle name flash now appears top-center under the wave text.
- **The ship is slightly smaller** (radius 15 → 13.5) — visuals and hitbox together, so it's a touch more
  forgiving in tight fields.
- **Optimized for any screen size.** The game now renders at a consistent apparent size on every monitor.
  The camera scale-to-fits a fixed design height to the window, so a bigger screen *magnifies* the action
  instead of revealing a vast, sparse empty arena (the cause of it "looking odd" on larger displays). The
  arena fills the width at the screen's aspect (no letterbox, no stretch), the starfield fills the whole
  screen (no dark margins on big / ultrawide monitors), and the HUD scales with the window so it's never tiny.
- **Life (gold) rocks retuned.** No more useless wave-1 life rock — the first now arrives in wave 2 — and
  they're **more frequent through the early-mid game** (waves 2–16, when a spare life matters most),
  **tapering back to rare by the wave-30 finale**.
- **Life rocks spark gold, not blue.** Shattering a gold rock now throws warm-gold debris; it was
  incorrectly spraying the default blue rock color.
- **Brighter grid shimmer.** The slow shimmer wave that sweeps across the grid now peaks much brighter, so
  the lit crests read clearly against the faint backdrop (the ~0.2 Hz sweep rate is unchanged — photosafe).

## v0.4.2 — Bigger small targets (2026-07-28)

- **Small things are bigger now — and the aim assist is gone.** The v0.4.1 aim assist didn't feel good (it
  played the game for you), so it's **removed**. Instead, the targets that were fiddly to hit simply grew
  ~35–40%: **small asteroids** (radius 22 → 30), **mines** (13 → 18), and **mobs** (14 → 19). They read
  clearer and take a shot without pixel-perfect aim — while aiming stays a pure skill (no snapping, no
  auto-lead). Nothing got more dangerous: a mine's kill still comes from its blast radius, not its body, and
  mobs still only threaten with their shots.

## v0.4.1 — Aim assist (2026-07-23)

- **Slight aim assist.** Small targets are hard to hit dead-on, so a shot now **snaps onto a target when
  your aim is already within a few degrees of it** (and it's within the bullet's range) — a subtle
  forgiveness on asteroids, enemies, and the possessed vessel. You still have to be nearly on it; it won't
  reach across the screen or lock onto anything you aren't already pointing at.

## v0.4.0 — The Haunt reforged: Possession, a cinematic finale, and a safety pass (2026-07-23)

Finale-fight fixes and polish (playtest feedback):

- **Phantom Phase 2 is now POSSESSION — a whole new mechanic** (replacing the old "identical decoys" shell
  game). The Haunt **seeks out a real asteroid, dives into it**, and hides inside — that rock turns haunted
  (glowing, ember-eyed), **homes at you, and kills on contact**, while your shots hit the *rock*, not the
  ghost. **Break the possessed rock** and the Haunt is **ripped out into the open** — surfaced and
  vulnerable, the punish window — then it hunts the next rock, until the phase falls. Each phase now has its
  own identity: **P1** the sweep ray, **P2** the possession hunt, **P3** the charge. (The one fight where the
  asteroid field itself becomes the boss.)
- **Phase 3 drops the beam — a pure, relentless charger now.** No more sweep ray in P3 (that's P1's
  signature); the cornered Haunt just **hurls itself at you on telegraphed lock-on lines**, more often the
  closer it is to death, and **stalks** you between lunges.
- **The sweep ray now reaches the whole arena AND aims at you.** It was falling short of the far edge from an
  off-centre position — the beam now spans the full arena diagonal from wherever it stands, and its swept
  quadrant **centres on the corner you're nearest**, so it's a real threat to dodge (the ~1.7s telegraph is
  your window).
- **The Haunt has its own sound now.** It was borrowing the warp effect for *everything*; every spectral cue
  (the ray igniting, possessing, being ripped out, charging, dying) is now its own eerie whisper-whoosh, not
  the warp.
- **It no longer clips through asteroids.** The Haunt's spectral body now **dissolves any rock it drifts
  through** (it's a ghost — matter unmakes in its wake), instead of overlapping them.
- **A cinematic death scene, reworked.** On the kill the boss first **draws back to the centre**, then
  **erupts** — an opening bang followed by a **constant stream of explosions from the middle that keeps the
  screen full of light** until the send-off ends (not a single split-second pop). Its **true-form core
  streaks off east**, small and subtle, easy to miss among the light (the sequel seed). Once the core has
  left the arena, **your ship warps off east after it** — and only once *everything* is cleared does the
  **Victory screen** begin. (No more stray "NEXT WAVE IN" on the win, no early Victory pop; the ship stays
  shielded throughout so a last-life win can't flip to Game Over.)
- **The finale field no longer arrives as a wall.** Each mono-type group of ten now **trickles in a rock at
  a time** instead of all appearing together, so a fresh colour drifts on gradually. (Also fixed: a lingering
  **gold 1UP rock** no longer stalls the finale — the belt keeps flowing while it's on screen.)
- **Red rocks work in the all-red finale group.** A red now absorbs the nearest rock **including other reds**,
  so a mono-type red pack **consolidates into fewer, bigger threats** instead of drifting inert (a pair can't
  annihilate each other — one grows, one is eaten).
- **Photosensitivity pass (accessibility).** Audited every light effect and kept all flashing/pulsing at
  **≤3 flashes per second** (the seizure-safety guideline): the full-screen boss-warning tint now *breathes*
  slowly (~0.7 Hz) instead of strobing on/off; the Devourer's "about to burst" white-hot flash (up to ~10 Hz
  before), the Phantom's ray/aim telegraphs, the warp grid crackle, and the death-scene explosion stream are
  all under the threshold; the ship / mine / HUD blinks are ≤3 Hz. Urgency still ramps — via how bright/white
  a flash gets, not how fast it flickers.
- **Launcher:** the VIOLET EDGE logo is now bundled as the launcher's card art (takes effect at the next
  launcher build).

## v0.3.0 — The Haunt: Act III, the six-boss run & the finale (2026-07-23)

The standard run is now complete end-to-end — **thirty waves, six bosses, and a final boss.**

- **The wave-30 finale is THE PHANTOM — reborn as THE HAUNT** (the channel-the-old-bosses design is out): a
  spectral predator **too arrogant to be touched**, with its own mechanics per phase. The fight is you
  stripping that arrogance away. The deliberate *exception* to "asteroids are the star."
  - **It's INTANGIBLE** — your shots pass straight through, and its ghost-body drifts harmlessly through you.
    **Firing its Sweep Ray forces it to SURFACE**: for a short window after each beam it's solid, still, and
    hittable (and lethal to touch). **Bait the ray, punish the recovery** — that's the fight.
  - **Per-phase health with a RESET:** deplete a phase → it reforms and the next begins. Three phases; the
    **Sweep Ray** (telegraphed quadrant → lethal sweeping beam) runs through all of them, faster each.
  - **Phase 1 — HAUNT:** the ghost + the ray. Learn the rhythm.
  - **Phase 2 — SPLIT:** it **fractures into identical apparitions**. The decoys roam and shimmer exactly
    like the real one and never attack — **only the real one fires (watch for the blazing eyes)**, and after
    every surface window it **shuffles places with a decoy**, so you can't camp it.
  - **Phase 3 — HUNT:** cornered, **the mask drops** — it turns solid full-time (always hittable, kills on
    contact) and **charges across the arena** on a telegraphed lock-on line, **searing a wake of spectral
    afterimages** that linger and kill on touch — the arena shrinks around you while the ray fires at its
    fastest.
  - **The look:** a menacing **spectral mask-visage** — an elongated angular skull in two halves, a heavy
    glaring brow, downward **eye-slashes** with red embers, **jagged fangs**, wreathed in a slow **broken
    halo/crown**. **Ghostly-faint and wavering** while intangible; when it surfaces, the **mask SPLITS OPEN
    around a searing white-hot core** (sealing as your window closes — you read the boss's own form, not a UI
    ring); a flash on each phase break; **three phase pips** by its health bar.
  - **A real send-off:** the finale kill no longer cuts in abruptly — a short **death-throes beat** plays (the
    arena goes calm, the core comes apart) and a **small spectral shard tears free and streaks off-screen**
    before the Victory screen.
  - **The finale field arrives in mono-type GROUPS of ten** — ten blue drift in; once the field is clear,
    ten green, then orange, then pulser, then red, and around again. Far less crowded, one colour at a time.
- **Finale fixes + code-health pass** (from an adversarial review):
  - The **win is now truly guaranteed** — a last-life death landing on the exact frame the Phantom dies can
    no longer flip your victory into a Game Over.
  - **Beating the Phantom awards score** now (it was the only boss kill worth 0 points).
  - The **ally Drone no longer targets the Phantom** — it was firing at the intangible ghost and, worse,
    its tracers gave away the real skull among the phase-2 decoys. The finale is your fight.
  - A **live gold 1UP no longer vanishes into the wave-30 slate-wipe** (which used to read as "cleared" and
    hand you a free life at the door of the finale).
  - **Boss music no longer loops over the Victory / menu screens** after a win.
  - Dev keys are gated to gameplay (no more F3 leaking a rock onto the menu); F2 reliably ends the fight.
  - Removed dead code + stale comments left by the finale's earlier designs.
- **Dev F4 — face the Phantom:** jumps straight to the wave-30 finale (wipes the field but keeps your ship,
  then spawns a fresh Phantom) so the final boss can be tested without clearing 29 waves. Debug builds only;
  pair with **F1** (invincibility). *(F1 invincible · F2 wave-skip · F3 spawn-orange · F4 face-the-Phantom.)*
- **The ally Drone now fires at bosses too**, not just asteroids — it targets the nearest asteroid *or*
  boss in range (except the finale Phantom), so it helps in boss fights (where the field is sparse).

- **Shot modes are now a 3-way cycle (Q):** Standard → Mass → Warhead (through whatever you've unlocked).
  - **Mass** is no longer an instant wipe — it's just **stronger** (`MASS_POWER=3`: one-shots a dense rock,
    which then splits normally).
  - **Warhead** is now a **toggle** and *the* instant-destroy tool: a **piercing** round that **passes
    through asteroids**, deleting each one it touches, keeping its **violet blast ring** — and still **no
    chaining**.
  - The ally **Drone** fires plain standard bullets, so it can never be a Warhead machine (no special-case
    needed anymore).
- **Dev F2** steps **one wave at a time** again (no longer jumping straight to bosses) and reliably kills
  every boss type along the way.

- **More playtest tuning + fixes:**
  - **Boss HP now ascends** wave-to-wave — 26 / 34 / 40 / 46 / 52 / 60 (Warden → Singularity). *(This drops
    the Glutton from 70; it's a healer, so it still plays tanky — say the word to keep it beefier.)*
  - **Bosses never touch the gold 1UP:** the Warden won't shield it, the Slinger won't tractor it, the
    Detonator won't prime it, the Pulsar won't fling it, the Singularity won't crush it (the Glutton already
    ignored it).
  - **Warhead nerfed** — its blast is now LOCAL (~110px vs the orange 250) and **no longer chains**, so
    spraying shots doesn't clear the screen. And the **ally Drone's shots are exempt** from Warhead.
  - **Victory screen reveals slowly**, credits-style (lines fade in on a stagger) instead of popping in.
  - **Gravity-Well render** redesigned into a whirlpool — it was a 4-arm cross that read like a swastika.

- **Playtest fixes + finale hardening (waves 12–30):**
  - **Limpets leave the arena** once their waves (12–13) are over, instead of lingering into later waves.
  - Detonator's **primed rocks are now hot red** (were gold — read like the 1UP).
  - **Red asteroids recolored** to a cool crimson (clearly apart from orange), and they no longer throw
    blue sparks or split into blue rocks — a red stays red from every weapon and can grow back to large.
  - **Pulsar (boss 5) is meaner** — stronger, more frequent shockwaves that shove rocks (and you) harder.
  - **Singularity redesigned** — the 3-arm spiral (too swastika-like) is now a 7-arm whirlpool, with a
    stronger, wider pull.
  - **Dev F2** now skips through every boss to the finale (it was stalling at wave 20).
  - **Finale hardening (from an adversarial review):** killing the Singularity now **wins instantly** (a
    stray rock during its death throes could previously flip the win into a Game Over); quitting/restarting
    mid-boss-fight no longer leaves a boss alive (a stale one could fire a false victory next run); and it
    no longer takes "fed-an-orange" damage during its intro invulnerability.

- **The finale is in — the run is beatable start to finish.** Wave 30 is now the **Singularity** (boss 6):
  a gravity core that drags every rock and your ship toward it. Chip its core while you fight the pull, or
  **feed it an orange** (let one get pulled in) for big damage; contact crushes you. Beating it triggers a
  **Victory** screen — *"YOU SAVED THE PLANET"* — that teases the New Game+ unlock. Six bosses, waves 1–30.
  (The boss powerup drops — Nova & Magnet — and difficulty tuning are the remaining work.)

- **The Pulsar (boss 5, wave 25) is in.** An electric white-cyan core that's **invulnerable while lit /
  open on the dark beat**, and on a beat it **shockwaves every rock and your ship outward** — shoot it in
  a dark window and don't get pinned to a wall. (Its Nova-pulse drop and the wave-30 Singularity are next.)

- **Act III begins — the run now reaches wave 30.** The wave engine authors content through **wave 30** —
  the standard run's full six-boss arc, ending there (…Detonator, then Pulsar at 25 and Singularity at 30
  — *bosses still placeholders, landing next*). **Green asteroids retire** across Act III and **orange +
  pulser become the standard field.** New **Red (growing)** asteroids debut in Act III: they absorb nearby
  rocks to swell, and a plain shot splits one into more reds (whack-a-mole) — mass / warhead / chain / mine
  clear them outright.

- **The Detonator — boss 4, wave 20** (closes out Act II). It's **armored except while it PRIMES a
  rock**: it drifts in to a rock, halts, and **beams it** (the beam shows which rock), its chartreuse core
  opening for ~1.5s — that channel is your only damage window. Each primed rock becomes a **live bomb**
  you must dodge. Wave 20 is now all-orange (the bombs it primes). Unique colour: hazard chartreuse.
- **Warhead rounds** (the Detonator's drop) — a permanent passive: every primary shot makes the rock it
  hits **detonate and chain**. The blast is **violet and safe to you** (your own explosions no longer kill
  you) — distinct from the orange, lethal bombs the boss and rocks throw. Echoes the primed bombs.
- **Mass shot reworked** — it now **destroys any asteroid in one hit, with no chunks left**, making it a
  genuine field-clearing tool instead of a slower standard shot. Lit (white, invulnerable) pulsers still
  shrug it off. Against bosses it's only a bit stronger than standard per hit, so its slow fire rate keeps
  the standard shot the better boss DPS.
- **Boss run-up warning** — the 10s before a boss now names the incoming boss on screen ("WARNING:
  THE WARDEN INCOMING") and pulses a full-screen tint in that boss's colour, rising in intensity as the
  wave nears. The faint background cameo silhouette is no longer the only telegraph. The in-fight HUD
  line also names the boss now (e.g. "WAVE 10    THE GLUTTON") instead of a generic "BOSS".
- **Mines toned down** — they no longer scale to a wall: fewer per wave, capped at 30% of the rock
  count and a hard cap of 6, so they stay a garnish instead of a constant swarm.

## v0.2.7 — Volatile: waves 16–20, the Slinger, pulsers & wells (2026-07-21)

The full Act II arc (waves 11–20) is now in.

**New content**
- **The Slinger** — wave-15 boss. A large ice-blue gunship that hovers high, tractor-beams a field
  rock to its muzzle, and fires it at you like a cannonball. Dodge the shot or shoot the loaded rock;
  chip its exposed core. Drops the **Drone**.
- **Drone** — the Slinger's pickup: an ally craft that follows you and mops up rocks you miss.
- **Pulser asteroids** (waves 16–20) — pulse bright white and are **invulnerable while lit**; hit them
  on the dark beat. They split into smaller pulsers. Wave 16 is pulser-only.
- **Gravity Well** (waves 18–19) — an "opposite warp" hazard that pops in at random and drags your
  ship. Weaker than your thrust, so you can always fly out.

**Changes**
- No blue asteroids past wave 10 (they harden to green).
- Non-boss waves shortened 180s → 120s (reaching wave 15 is ~28 min, not ~40).
- Post-boss flow: a "NEXT WAVE IN n" countdown, then the WAVE banner (no more overlap).
- The boss run-up now previews the *actual* incoming boss, and stray mobs retreat off-screen first.
- Warping the gold 1UP grants the life (it's a player action).
- Dev **F2** wave-skip now works on every boss wave.
- Fixes/polish: Slinger sparse field, pulser sparks white, rocks dissolve when a boss clears the
  field, Devourer size floor, orange blast VFX, persistent shot-mode indicator.
- CI: macOS `.dmg` "no space" fixed (strip the binary + free cargo caches).

## v0.2.6 — Controls, waves 11–15, the Limpet & Glutton (2026-07-20)

- **Waves 11–15** as bespoke content: orange **explosive** asteroids wired into the field (all-orange
  wave 14), and **The Limpet** — a parasite mob (waves 12–13) that tethers to a rock and peeks out to
  fire; break the rock or flank it.
- The **Glutton** (wave-10 boss) now starts at full health and heals less; the **warp** grid glows and
  crackles and its pull is stronger.
- **Controls screen** does full input rebinding for keyboard/mouse *and* controller (the separate
  Settings screen was merged in).
- CI emits launcher-compatible native assets; wired into the Baz Studios launcher.

## v0.2.5 — Controller support (2026-07-20)

- Play with a controller *or* keyboard/mouse (both live at once); input-method auto-detect + a full
  rebinding screen.

## v0.2.0–v0.2.4 — Initial native port (2026-07-17–18)

- First GitHub releases of the Rust + Bevy port (from the JS/Canvas original): core Asteroids loop,
  the Warden (w5) and Glutton (w10) bosses, chain + mass pickups, the gold 1UP economy, top-5 high
  scores, menus/achievements, procedural audio.
- Release pipeline: CI builds Windows / macOS / Linux; macOS ships a real `VIOLET EDGE.app` in a
  drag-to-Applications `.dmg`; embedded exe icon + window logo.
