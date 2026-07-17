//! VIOLET EDGE — Bevy port. (Renamed from NEON EDGE, which was taken.)
//!
//! Vertical slice + juice: window, neon grid (dim, shimmering) over a starfield,
//! a ship you rotate/thrust/fire (Space or left-click) with a thrust flame, blue
//! asteroids with elastic physics that split when shot, particle bursts, bullet
//! trails, ship death → respawn (with invuln), a lives HUD, and Pause + Game-Over
//! screens driven by a Bevy state machine.
//!
//! Rendering: Bevy gizmos (immediate-mode wireframes) on an HDR camera with Bloom
//! for the glow. UI text via bevy_ui (default font). Written against Bevy 0.16.

// Bevy ECS systems idiomatically take many query params, and its query types are
// verbose by nature — clippy's `too_many_arguments`/`type_complexity` are noise here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;

mod audio;
use rand::Rng;
use std::collections::HashSet;

// ─────────────────────────────── config ───────────────────────────────
const TAU: f32 = std::f32::consts::TAU;

const SHIP_R: f32 = 15.0;
const TURN_RATE: f32 = 3.6; // rad/s
const THRUST: f32 = 620.0; // px/s^2
const FRICTION: f32 = 0.55; // velocity kept per second
const MAX_SPEED: f32 = 560.0; // px/s
const FIRE_COOLDOWN: f32 = 0.18; // s

const BULLET_SPEED: f32 = 720.0; // px/s
const BULLET_LIFE: f32 = 1.6; // s
const BULLET_R: f32 = 3.0;

const GRID_CELL: f32 = 52.0;
const WAVE_SECS: f32 = 180.0; // 3-minute waves — survive the timer to advance
const POP_BASE: i32 = 5; // asteroids on screen = POP_BASE + wave...
const POP_CAP: i32 = 18; // ...capped so the field never becomes an unavoidable wall
const BIG_FLOOR: i32 = 3; // always keep at least this many LARGE (size-3) rocks around: keeps the
                          // field from silting up with small debris, and gives the boss big rocks to grab
const SPAWN_INTERVAL: f32 = 1.6; // seconds between streamed-in replacement rocks (manageable rate)

const WARP_MAX_CHARGES: i32 = 3; // fire all 3, THEN the long cooldown refills them together
const WARP_COOLDOWN: f32 = 35.0; // long refill once all charges are spent — not spammable
const WARP_MISSILE_SPEED: f32 = 430.0;
const WARP_MISSILE_LIFE: f32 = 1.3; // flies farther before tearing open the hole
const WARP_HOLE_LIFE: f32 = 2.6;
const WARP_PULL_RADIUS: f32 = 500.0; // a bit bigger than the old 440 (JS 360 read too small
// with our longer missile throw); still well short of arena-spanning (~755 was too far)
const WARP_PULL: f32 = 2000.0; // much more aggressive inward yank than JS (was 900)
const WARP_CONSUME_R: f32 = 120.0; // event horizon — anything whose EDGE crosses this is
// instantly destroyed (rocks/enemies/mines), like a real black hole. Big enough that pulled-in
// rocks are eaten on contact instead of clumping + colliding around a tiny mouth.
const WARP_GRID_RADIUS: f32 = 340.0; // the grid bends toward the hole within this
const WARP_GRID_STRENGTH: f32 = 82.0; // max inward grid displacement (px) at the hole
const WARP_SNAP_DUR: f32 = 0.7; // rubber-band snapback time after the hole closes

// Mines (wave 2+): drifting proximity crimson mines.
const MINE_FIRST_WAVE: i32 = 2;
const MINE_PER_WAVE: i32 = 2; // target = (wave - first + 1) * per...
const MINE_MAX_FRACTION: f32 = 0.5; // ...never more than this fraction of the asteroid count
const MINE_R: f32 = 13.0;
const MINE_SPEED: f32 = 62.0; // px/s drift
const MINE_TRIGGER_R: f32 = 92.0; // ship within → the mine arms (blinks)
const MINE_BLAST_R: f32 = 52.0; // armed + ship within → detonate (kills the ship)
const MINE_FUSE: f32 = 0.6; // arming time before it can detonate (time to escape)
const MINE_SCORE: u32 = 150;
const MINE_SPAWN_INTERVAL: f32 = 2.6;
const MINE_CHUNK_MULT: f32 = 1.9; // HIDDEN: rocks shattered by a mine blast fling chunks this much faster

// Enemy ships (wave 3+): drift in, hover-and-strafe while firing at the ship, dodge
// mines/rocks, get sucked into the warp, and bug out if they linger too long.
const ENEMY_FIRST_WAVE: i32 = 3;
const ENEMY_LAST_WAVE: i32 = 5; // yellow mobs stop after this (wave 6+ = dense rocks instead)
const ENEMY_PER_WAVE: i32 = 2; // target = (wave - first + 1) * per...
const ENEMY_MAX_FRACTION: f32 = 0.3; // ...capped well below the rock count (a garnish)
const ENEMY_R: f32 = 14.0;
const ENEMY_MAX_SPEED: f32 = 125.0; // px/s
const ENEMY_ACCEL: f32 = 640.0; // px/s² steering force
const ENEMY_PREF_DIST: f32 = 260.0; // hovers around this range from the ship
const ENEMY_AVOID_R: f32 = 95.0; // steer away from mines/rocks within this
const ENEMY_SEP_R: f32 = ENEMY_R * 4.0; // steer away from EACH OTHER within this (no stacking)
const ENEMY_FIRE_EVERY: f32 = 2.4; // s between shots (deliberately slow, dodgeable)
const ENEMY_FIRE_JITTER: f32 = 0.9;
const ENEMY_BULLET_SPEED: f32 = 250.0; // px/s (ship is faster, so it's dodgeable)
const ENEMY_BULLET_R: f32 = 5.0;
const ENEMY_BULLET_LIFE: f32 = 4.5; // s
const ENEMY_LIFETIME: f32 = 11.0; // s on-screen before it flees (never overstays)
const ENEMY_SCORE: u32 = 300;
const ENEMY_SPAWN_INTERVAL: f32 = 3.0;

// Dense (green) asteroids — introduced from wave 6 (replacing the yellow mobs). They
// take multiple bullet hits to crack (hp = size); chain/mine still break them at once.
const DENSE_FIRST_WAVE: i32 = 6;
const DENSE_FRACTION: f64 = 0.5; // chance a wave-6+ edge spawn is dense

// Octopus boss (every 5th wave): a magenta core that captures field asteroids into a
// rotating orbital shield (its "arms") and hurls the smallest held rocks at the ship.
const BOSS_WAVE_INTERVAL: i32 = 5; // waves 5, 10, 15, … are boss waves
const BOSS_R: f32 = 38.0;
const BOSS_HP: i32 = 28; // core hits to kill (the shield blocks most shots)
const BOSS_ARMS: usize = 6; // asteroids it can hold at once
const BOSS_ORBIT_R: f32 = 132.0; // arm length — how far the shield orbits
const BOSS_SPIN: f32 = 0.85; // rad/s shield rotation
const BOSS_GRAB_TIME: f32 = 1.8; // s a grabbed rock reels into its slot (slow + telegraphed)
const BOSS_CAPTURE_EVERY: f32 = 1.1; // s between grabs (deliberately unhurried)
const BOSS_ENTER_SPEED: f32 = 320.0; // px/s glide-in from the top
const BOSS_FIRE_EVERY: f32 = 2.0; // s throw cadence
const BOSS_FIRE_JITTER: f32 = 0.7;
const BOSS_THROW_SPEED: f32 = 280.0; // px/s of a hurled rock
const BOSS_CHARGE: f32 = 1.4; // s power-up after entering (invulnerable)
const BOSS_DEATH_SECS: f32 = 2.2; // slow death animation before it despawns
const BOSS_CALM: f32 = 10.0; // s post-kill lull before the next wave (the pickup window)
const BOSS_SCORE: u32 = 3000;
const BOSS_CAMEO_SECS: f32 = 10.0; // boss drifts by in the background this long before its wave

// Chain shot: a wide lightning BEAM secondary weapon. Unlocked by the pickup that
// appears in the calm after the first boss (wave 5). 3 charges that regenerate.
const CHAIN_MAX_CHARGES: i32 = 3;
const CHAIN_RECHARGE: f32 = 5.5; // s to regenerate one charge
const CHAIN_COOLDOWN: f32 = 0.27; // min s between shots
const CHAIN_SPEED: f32 = 540.0; // px/s
const CHAIN_HALF: f32 = 58.0; // half the beam width (gap between the two chained ends)
const CHAIN_R: f32 = 8.0; // beam hit half-thickness
const CHAIN_LIFE: f32 = 1.5; // s
const PICKUP_R: f32 = 30.0; // reward-orb radius
const PICKUP_DRIFT: f32 = 32.0; // px/s slow drift
const PICKUP_LIFE: f32 = 20.0; // the orb lingers this long (well past the 10s boss calm) before vanishing

const MAX_SEP: f32 = 6.0; // px/frame cap on overlap push-out
const RESTITUTION: f32 = 1.0; // fully elastic bounce
const MIN_DRIFT: f32 = 30.0; // px/s — rocks never fully stop (elastic hits can zero them → "stuck")

const RESPAWN_DELAY: f32 = 1.3; // s the ship stays gone after dying
const SPAWN_INVULN: f32 = 2.0; // s of blink-invulnerability on (re)spawn
const TRAIL_LEN: usize = 10; // bullet trail points kept
const STAR_COUNT: usize = 90;
const START_LIVES: i32 = 3;
const WAVE_BANNER_SECS: f32 = 2.4; // how long the big "WAVE n" flash lingers
const WAVE_BANNER_FADE: f32 = 1.2; // of that, the trailing fade-out duration

// Bright (>1.0) colors so the HDR camera's bloom makes them glow.
fn ship_color() -> Color {
    Color::srgb(3.2, 0.7, 6.5)
} // neon violet — the player + its kit (bright peak so bloom keeps it glowing, not dark)
fn flame_color() -> Color {
    Color::srgb(3.2, 1.7, 5.0)
} // hot purple-white exhaust
fn bullet_color() -> Color {
    Color::srgb(2.4, 1.0, 4.6)
}
fn rock_color() -> Color {
    Color::srgb(0.3, 2.4, 5.0)
} // neon blue
fn dense_color() -> Color {
    Color::srgb(0.5, 5.0, 1.4)
} // neon green — dense (tanky) asteroids
fn grid_color() -> Color {
    Color::srgb(0.02, 0.06, 0.2)
} // faint backdrop
fn star_color() -> Color {
    Color::srgb(0.5, 0.7, 1.15)
}
fn warp_color() -> Color {
    Color::srgb(2.6, 1.2, 5.0)
} // warp purple (player kit)
fn mine_color() -> Color {
    Color::srgb(5.0, 0.7, 1.7)
} // hot crimson = danger
fn enemy_color() -> Color {
    Color::srgb(5.0, 3.6, 0.5)
} // neon yellow — enemy ships + their shots
fn boss_color() -> Color {
    Color::srgb(5.0, 1.6, 4.1)
} // neon magenta — the boss
fn chain_color() -> Color {
    Color::srgb(3.4, 2.0, 5.6)
} // electric violet lightning — the chain shot (player kit)

fn mine_target(level: i32, asteroids: i32) -> i32 {
    if level < MINE_FIRST_WAVE {
        return 0;
    }
    let raw = (level - MINE_FIRST_WAVE + 1) * MINE_PER_WAVE;
    raw.min((asteroids as f32 * MINE_MAX_FRACTION) as i32)
}

fn asteroid_radius(size: u8) -> f32 {
    match size {
        3 => 88.0, // LARGE
        2 => 46.0, // MID
        _ => 22.0, // SMALL
    }
}
fn body_mass(r: f32) -> f32 {
    r * r
}
fn population_target(level: i32) -> i32 {
    (POP_BASE + level).min(POP_CAP)
}

/// Elastic collision between two circular bodies: separate out of overlap
/// (capped) and exchange momentum along the normal. Ported from JS collideAsteroids.
fn resolve(pa: &mut Vec2, va: &mut Vec2, ma: f32, ra: f32, pb: &mut Vec2, vb: &mut Vec2, mb: f32, rb: f32) {
    let delta = *pb - *pa;
    let d2 = delta.length_squared();
    let min = ra + rb;
    if d2 >= min * min || d2 == 0.0 {
        return;
    }
    let dist = d2.sqrt();
    let n = delta / dist;
    let total = ma + mb;
    let corr = (min - dist).min(MAX_SEP);
    *pa -= n * (corr * mb / total);
    *pb += n * (corr * ma / total);
    let vn = (*vb - *va).dot(n);
    if vn > 0.0 {
        return;
    }
    let j = -(1.0 + RESTITUTION) * vn / (1.0 / ma + 1.0 / mb);
    *va -= n * (j / ma);
    *vb += n * (j / mb);
}

/// Scale a color's brightness (fade). With bloom, dimming kills the glow too.
fn dim(color: Color, f: f32) -> Color {
    let s = color.to_srgba();
    Color::srgb(s.red * f, s.green * f, s.blue * f)
}

// Linear blend between two colors (t: 0 → a, 1 → b). Used for gradient effects
// like the bullet flame (deep purple tip → hot base).
fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    Color::srgb(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
    )
}

// Inward pull a black hole applies to a body at `pos` this frame (velocity delta).
// Zero outside `pull_r`; a strong floor at the rim, ramping harder toward the core.
// Shared by every hazard the warp drags in (rocks, enemies, mines).
fn warp_pull(pos: Vec2, hole: Vec2, pull_r: f32, dt: f32) -> Vec2 {
    let d = pos.distance(hole);
    if d >= pull_r || d < 1.0 {
        return Vec2::ZERO;
    }
    let dir = (hole - pos) / d;
    let falloff = 1.0 - d / pull_r;
    dir * (WARP_PULL * (0.35 + 0.65 * falloff)) * dt
}

// Squared distance from point `p` to segment `a`–`b` (for chain-beam vs target hits).
fn seg_dist2(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length_squared()
}

/// Rubber-band ease: 0→1 with a decaying overshoot past 1 (used for the snapback).
fn ease_out_elastic(p: f32) -> f32 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let c4 = (2.0 * std::f32::consts::PI) / 3.0;
    2.0_f32.powf(-10.0 * p) * ((p * 10.0 - 0.75) * c4).sin() + 1.0
}

/// Displace a point toward the warp hole (or push it out when `amount` is negative
/// during the snapback overshoot). Falls off to nothing at WARP_GRID_RADIUS.
fn warp_point(p: Vec2, wf: &WarpField) -> Vec2 {
    if wf.amount == 0.0 {
        return p;
    }
    let to = wf.pos - p;
    let d = to.length();
    if !(1.0..WARP_GRID_RADIUS).contains(&d) {
        return p;
    }
    let fall = 1.0 - d / WARP_GRID_RADIUS;
    p + (to / d) * (wf.amount * WARP_GRID_STRENGTH * fall * fall)
}

/// Spawn a spray of fading particles from `pos`.
fn burst(commands: &mut Commands, pos: Vec2, color: Color, count: usize, speed: f32, rng: &mut impl Rng) {
    for _ in 0..count {
        let a = rng.gen_range(0.0..TAU);
        let s = rng.gen_range(speed * 0.35..speed);
        let ttl = rng.gen_range(0.3..0.75);
        commands.spawn((
            Particle { vel: Vec2::from_angle(a) * s, life: ttl, ttl, color },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }
}

// The shared ship-death sequence: debris burst, despawn, lose a life → respawn or
// (on the last life) Game Over. Used by ship_death (asteroids) and mine_update.
fn kill_ship(
    commands: &mut Commands,
    run: &mut Run,
    next: &mut NextState<GameState>,
    sfx: &mut EventWriter<SoundFx>,
    ship_e: Entity,
    pos: Vec2,
    rng: &mut impl Rng,
) {
    burst(commands, pos, ship_color(), 30, 340.0, rng);
    burst(commands, pos, Color::srgb(4.0, 4.0, 5.0), 12, 220.0, rng);
    sfx.write(SoundFx::Death);
    commands.entity(ship_e).despawn();
    run.lives -= 1;
    if run.lives <= 0 {
        next.set(GameState::GameOver);
    } else {
        run.respawn = RESPAWN_DELAY;
    }
}

// A combat kill of an enemy mob: award score, splash debris, play the death zap, despawn.
// Shared by the bullet hit and the chain-beam hit so the two can't drift apart.
fn kill_enemy(commands: &mut Commands, score: &mut Score, sfx: &mut EventWriter<SoundFx>, e: Entity, pos: Vec2, rng: &mut impl Rng) {
    score.0 += ENEMY_SCORE;
    burst(commands, pos, enemy_color(), 20, 320.0, rng);
    sfx.write(SoundFx::EnemyDie);
    commands.entity(e).despawn();
}

// ─────────────────────────────── state / components / resources ───────
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum GameState {
    #[default]
    Playing,
    Paused,
    GameOver,
}

#[derive(Component)]
struct Ship {
    angle: f32, // facing, radians (CCW from +X; +Y is up)
    cooldown: f32,
    invuln: f32, // spawn-protection seconds (blinks while > 0)
    flame: f32,  // 0..1 thrust flame intensity
}

#[derive(Component)]
struct Velocity(Vec2);

#[derive(Component)]
struct Asteroid {
    size: u8,
    verts: Vec<Vec2>,
    rot: f32,
    spin: f32,
    dense: bool, // green + tanky: takes `hp` bullet hits before it cracks
    hp: i32,     // bullet hits remaining (1 for normal rocks; = size for dense)
}

#[derive(Component)]
struct Bullet {
    life: f32,
    trail: Vec<Vec2>,
}

#[derive(Component)]
struct Particle {
    vel: Vec2,
    life: f32,
    ttl: f32,
    color: Color,
}

#[derive(Component)]
struct Star {
    phase: f32,
    bright: f32,
}

// A drifting proximity mine: arms when the ship is near, detonates after a fuse.
#[derive(Component)]
struct Mine {
    armed: bool,
    fuse: f32,
}

// An enemy ship: glides in, then hovers + strafes around the ship firing shots,
// until it's killed, sucked into a warp, or its lifetime runs out (then it flees).
#[derive(Component)]
struct Enemy {
    fire: f32,     // countdown to the next shot
    life: f32,     // time left before it bugs out
    strafe: f32,   // ±1 orbit direction
    entered: bool, // has it finished gliding onto the screen?
    fleeing: bool, // lifetime elapsed → heading for the nearest edge
}

// A slow enemy shot. Distinct from the player's `Bullet` (never purple).
#[derive(Component)]
struct EnemyBullet {
    life: f32,
}

// The octopus boss core.
#[derive(Component)]
struct Boss {
    hp: i32,
    rot: f32,     // shield rotation angle
    pulse: f32,   // visual throb + bob phase
    entered: bool, // finished gliding in?
    charge: f32,  // > 0 while powering up (invulnerable)
    fire: f32,    // countdown to the next throw
    capture: f32, // countdown to the next grab
    dying: f32,   // > 0 → death animation counting down; despawns at 0
}

// An asteroid captured onto the boss's shield (orbits slot `slot`; `grab` eases it in).
#[derive(Component)]
struct Shielded {
    slot: usize,
    grab: f32,
}

// A rock the boss just hurled — briefly un-grabbable so it can't be re-captured instantly.
#[derive(Component)]
struct Thrown(f32);

// A chain-shot beam: travels along `Velocity`; the damaging lightning spans `perp`·±half.
#[derive(Component)]
struct ChainShot {
    life: f32,
    perp: Vec2,
}

// The reward orb that drifts in the calm after the first boss — fly into it to unlock
// the chain shot, or leave it (hardcore). Only the chain kind exists so far.
#[derive(Component)]
struct Pickup {
    rot: f32,
    pulse: f32,
    life: f32, // seconds the orb lingers before it's gone for good (outlives the boss calm)
}

// UI markers (each overlay's root; despawned on state exit — despawn is recursive).
#[derive(Component)]
struct PauseUi;
#[derive(Component)]
struct GameOverUi;
#[derive(Component)]
struct WaveText; // top-center "WAVE n  M:SS"
#[derive(Component)]
struct ScoreText; // top-left "SCORE n"
#[derive(Component)]
struct WaveBannerText; // big center-screen "WAVE n" flash that fades out

// Warp: a slow missile that tears open a black hole which drags in + consumes rocks.
#[derive(Component)]
struct WarpMissile {
    life: f32,
}
#[derive(Component)]
struct BlackHole {
    life: f32,
    spin: f32,
}

#[derive(Resource, Default)]
struct Score(u32);

#[derive(Resource)]
struct Run {
    lives: i32,
    respawn: f32,
}

#[derive(Resource)]
struct Wave {
    level: i32,
    timer: f32,
    calm: f32, // > 0 during the post-boss calm — pauses spawns + the wave timer
}

// Counts down while the big "WAVE n" flash is on screen (0 = hidden).
#[derive(Resource, Default)]
struct WaveBanner {
    timer: f32,
}

// Throttles the streamed-in replacement asteroids so the field refills gradually.
#[derive(Resource, Default)]
struct SpawnClock(f32);

// Throttles mine spawns.
#[derive(Resource, Default)]
struct MineClock(f32);

// Throttles enemy-ship spawns.
#[derive(Resource, Default)]
struct EnemyClock(f32);

// Tracks the last boss wave a boss was spawned for, so exactly one spawns per wave.
#[derive(Resource, Default)]
struct BossState {
    fought: i32,
}

// Chain-shot state (the secondary weapon). `unlocked` flips when the pickup is grabbed.
#[derive(Resource, Default)]
struct Chain {
    unlocked: bool,
    charges: i32,
    recharge: f32, // countdown to regenerating one charge
    cooldown: f32, // min gap between shots
}

fn is_boss_wave(level: i32) -> bool {
    level % BOSS_WAVE_INTERVAL == 0
}

#[derive(Resource)]
struct Warp {
    charges: i32,
    cooldown: f32, // > 0 only while refilling after all charges were spent
}

// Drives the grid's pull-toward-hole warp + its elastic snapback. `amount` is the
// warp strength: eases 0→1 while a hole is open, then snaps 1→0 (overshooting
// negative = grid bulges out) over WARP_SNAP_DUR after it closes.
#[derive(Resource, Default)]
struct WarpField {
    pos: Vec2,
    active: bool,
    snap_t: f32,
    amount: f32,
}

#[derive(Resource)]
struct Arena {
    half: Vec2,
}

// ─────────────────────────────── setup / spawners ─────────────────────
fn setup(mut commands: Commands) {
    // HDR + bloom camera → the neon glow. (Global bloom stays at Bevy's default;
    // the warp shot glows harder via its own brighter HDR colors, not more bloom.)
    commands.spawn((
        Camera2d,
        Camera { hdr: true, ..default() },
        Tonemapping::TonyMcMapface,
        Bloom::default(),
    ));

    // starfield — fixed positions, each with its own twinkle phase
    let mut rng = rand::thread_rng();
    for _ in 0..STAR_COUNT {
        let pos = Vec2::new(rng.gen_range(-720.0..720.0), rng.gen_range(-460.0..460.0));
        commands.spawn((
            Star { phase: rng.gen_range(0.0..TAU), bright: rng.gen_range(0.3..1.0) },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }

    spawn_player(&mut commands);
    // The field starts EMPTY. `top_up_asteroids` drifts rocks in from the edges
    // (from frame one), so nothing ever spawns on top of the player.
}

// Persistent HUD. Lives label (top-right; the ship-icon count is drawn per-frame in
// `render`), score (top-left), and wave + timer (top-center).
fn spawn_hud(mut commands: Commands) {
    let label = Color::srgb(0.7, 0.85, 1.2);
    commands.spawn((
        Text::new("LIVES"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(label),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), right: Val::Px(22.0), ..default() },
    ));
    commands.spawn((
        ScoreText,
        Text::new("SCORE 0"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(label),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), left: Val::Px(22.0), ..default() },
    ));
    // centered wrapper so the wave/timer sits at the top-center
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                WaveText,
                Text::new("WAVE 1    3:00"),
                TextFont { font_size: 22.0, ..default() },
                TextColor(Color::srgb(0.85, 0.9, 1.2)),
            ));
        });
    // big center-screen "WAVE n" flash — alpha driven by wave_banner_update
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                WaveBannerText,
                Text::new(""),
                TextFont { font_size: 66.0, ..default() },
                TextColor(Color::srgba(0.8, 0.9, 1.3, 0.0)),
            ));
        });
}

fn spawn_player(commands: &mut Commands) {
    commands.spawn((
        Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: SPAWN_INVULN, flame: 0.0 },
        Velocity(Vec2::ZERO),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

// Should a rock spawned for `level` be the tanky green variant? From wave 6 on, half
// the edge spawns come in dense (single roll, shared by every edge-spawn caller).
fn roll_dense(level: i32, rng: &mut impl Rng) -> bool {
    level >= DENSE_FIRST_WAVE && rng.gen_bool(DENSE_FRACTION)
}

// A fresh large asteroid entering from just off a random edge (wave top-up). `dense`
// spawns the tanky green variant (the caller decides based on the wave).
fn spawn_edge_asteroid(commands: &mut Commands, half: Vec2, rng: &mut impl Rng, dense: bool, force_big: bool) {
    // mostly LARGE rocks (break into mid → small), with some MID ones mixed in. `force_big`
    // guarantees a LARGE one (used to refill the big-rock floor).
    let size = if force_big || rng.gen_bool(0.7) { 3 } else { 2 };
    let r = asteroid_radius(size);
    let inward = rng.gen_range(50.0..110.0);
    let jitter = rng.gen_range(-40.0..40.0);
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - r, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + r, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - r), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + r), Vec2::new(jitter, -inward)),
    };
    spawn_asteroid(commands, pos, size, vel, rng, dense);
}

// A jagged rock outline sized for `size` (regenerated when a shield rock shrinks).
fn asteroid_verts(size: u8, rng: &mut impl Rng) -> Vec<Vec2> {
    let r = asteroid_radius(size);
    let n = rng.gen_range(9..14);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let rr = r * rng.gen_range(0.72..1.12);
            Vec2::new(a.cos() * rr, a.sin() * rr)
        })
        .collect()
}

fn spawn_asteroid(commands: &mut Commands, pos: Vec2, size: u8, vel: Vec2, rng: &mut impl Rng, dense: bool) {
    commands.spawn((
        Asteroid {
            size,
            verts: asteroid_verts(size, rng),
            rot: rng.gen_range(0.0..TAU),
            spin: rng.gen_range(-0.8..0.8),
            dense,
            hp: if dense { size as i32 } else { 1 },
        },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

// Shatter one rock: despawn it, award score, splash debris, and (unless it's the
// smallest) split it into two smaller rocks flung outward. `chunk_mult` scales
// the child fling speed — 1.0 for a normal bullet break; a mine blast passes a
// bigger value so its chunks scatter faster (a discoverable interaction).
fn break_asteroid(commands: &mut Commands, rng: &mut impl Rng, score: &mut Score, e: Entity, pos: Vec2, size: u8, chunk_mult: f32, dense: bool) {
    commands.entity(e).despawn();
    let base = match size {
        3 => 20,
        2 => 50,
        _ => 100,
    };
    score.0 += if dense { base * 2 } else { base }; // dense rocks are worth more
    burst(commands, pos, if dense { dense_color() } else { rock_color() }, 10 + size as usize * 5, 260.0, rng);
    if size > 1 {
        // Split into two chunks that fly APART along a random axis. Each is spawned
        // already clear of the other (offset past their combined radii) so the pair
        // never overlaps — an overlapping spawn lets the collision resolver cancel
        // their motion and leaves them oozing apart at the break point instead of
        // shooting off. Headings get a little jitter so it isn't a rigid mirror.
        // Children inherit density (a dense rock breaks into dense chunks).
        let axis = rng.gen_range(0.0..TAU);
        let out = Vec2::from_angle(axis);
        let offset = asteroid_radius(size - 1) + 3.0;
        for side in [1.0f32, -1.0] {
            let spd = rng.gen_range(60.0..150.0) * chunk_mult;
            let vel = Vec2::from_angle(axis + rng.gen_range(-0.35..0.35)) * (side * spd);
            spawn_asteroid(commands, pos + out * (side * offset), size - 1, vel, rng, dense);
        }
    }
}

// A mine blast: break every rock within the blast radius, flinging chunks fast
// (MINE_CHUNK_MULT). Shared by EVERY way a mine goes off — shot, ship contact, or
// drifting into a rock — so the crowd-clear behaviour stays identical. `broken`
// guards against hitting the same rock twice when blasts overlap in one frame.
fn blast_asteroids(
    commands: &mut Commands,
    rng: &mut impl Rng,
    score: &mut Score,
    asteroids: &Query<(Entity, &Transform, &mut Asteroid), (Without<Mine>, Without<Shielded>)>,
    broken: &mut HashSet<Entity>,
    center: Vec2,
) {
    // shared &Query → iterates read-only, so we just read size/dense here
    for (ae, at, a) in asteroids {
        if broken.contains(&ae) {
            continue;
        }
        let ap = at.translation.truncate();
        let br = MINE_BLAST_R + asteroid_radius(a.size);
        if center.distance_squared(ap) < br * br {
            broken.insert(ae);
            break_asteroid(commands, rng, score, ae, ap, a.size, MINE_CHUNK_MULT, a.dense); // mine obliterates (ignores hp)
        }
    }
}

// ─────────────────────────────── gameplay systems (Playing only) ──────
fn ship_control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut q: Query<(&mut Ship, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (mut ship, mut vel, t) in &mut q {
        if keys.pressed(KeyCode::ArrowLeft) || keys.pressed(KeyCode::KeyA) {
            ship.angle += TURN_RATE * dt;
        }
        if keys.pressed(KeyCode::ArrowRight) || keys.pressed(KeyCode::KeyD) {
            ship.angle -= TURN_RATE * dt;
        }
        let thrusting = keys.pressed(KeyCode::ArrowUp) || keys.pressed(KeyCode::KeyW);
        if thrusting {
            vel.0 += Vec2::from_angle(ship.angle) * THRUST * dt;
            ship.flame = (ship.flame + dt * 5.0).min(1.0);
            // exhaust sparks straight out the BACK (opposite the nose) — NOT blended with
            // the ship's velocity, so it never sprays sideways when drifting. Sparse.
            if rng.gen_bool(0.3) {
                let back = ship.angle + TAU * 0.5 + rng.gen_range(-0.22..0.22);
                let tail = t.translation.truncate() - Vec2::from_angle(ship.angle) * SHIP_R * 0.5;
                commands.spawn((
                    Particle {
                        vel: Vec2::from_angle(back) * rng.gen_range(90.0..150.0),
                        life: 0.26,
                        ttl: 0.26,
                        color: flame_color(),
                    },
                    Transform::from_xyz(tail.x, tail.y, 0.0),
                ));
            }
        } else {
            ship.flame = (ship.flame - dt * 6.0).max(0.0);
        }
        vel.0 *= FRICTION.powf(dt);
        if vel.0.length() > MAX_SPEED {
            vel.0 = vel.0.normalize() * MAX_SPEED;
        }
        if ship.invuln > 0.0 {
            ship.invuln -= dt;
        }
    }
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sfx: EventWriter<SoundFx>,
    mut q: Query<(&mut Ship, &Transform)>,
) {
    let dt = time.delta_secs();
    let want_fire = keys.pressed(KeyCode::Space) || mouse.pressed(MouseButton::Left);
    for (mut ship, t) in &mut q {
        if ship.cooldown > 0.0 {
            ship.cooldown -= dt;
        }
        if want_fire && ship.cooldown <= 0.0 {
            ship.cooldown = FIRE_COOLDOWN;
            let dir = Vec2::from_angle(ship.angle);
            let pos = t.translation.truncate() + dir * SHIP_R;
            commands.spawn((
                Bullet { life: BULLET_LIFE, trail: Vec::new() },
                Velocity(dir * BULLET_SPEED),
                Transform::from_xyz(pos.x, pos.y, 0.0),
            ));
            sfx.write(SoundFx::Fire);
        }
    }
}

fn integrate(time: Res<Time>, mut q: Query<(&mut Transform, &Velocity)>) {
    let dt = time.delta_secs();
    for (mut t, v) in &mut q {
        t.translation.x += v.0.x * dt;
        t.translation.y += v.0.y * dt;
    }
}

fn bullet_trail(mut q: Query<(&Transform, &mut Bullet)>) {
    for (t, mut b) in &mut q {
        b.trail.push(t.translation.truncate());
        if b.trail.len() > TRAIL_LEN {
            let extra = b.trail.len() - TRAIL_LEN;
            b.trail.drain(0..extra);
        }
    }
}

// Asteroids bounce off each other (elastic), never interpenetrate.
fn asteroid_collisions(mut q: Query<(&mut Transform, &mut Velocity, &Asteroid), Without<Shielded>>) {
    let mut it = q.iter_combinations_mut::<2>();
    while let Some([(mut ta, mut va, aa), (mut tb, mut vb, ab)]) = it.fetch_next() {
        let (ra, rb) = (asteroid_radius(aa.size), asteroid_radius(ab.size));
        let mut pa = ta.translation.truncate();
        let mut pb = tb.translation.truncate();
        resolve(&mut pa, &mut va.0, body_mass(ra), ra, &mut pb, &mut vb.0, body_mass(rb), rb);
        ta.translation.x = pa.x;
        ta.translation.y = pa.y;
        tb.translation.x = pb.x;
        tb.translation.y = pb.y;
    }
}

// Dev-only invincibility (toggled with F1 — see `dev_toggle`). The resource always
// exists so the death checks can read it cheaply; only the TOGGLE is compiled into
// debug builds, so a release build can never flip it on.
#[derive(Resource, Default)]
struct Dev {
    invincible: bool,
}

// A ship shrugs off lethal hits while blinking after (re)spawn, OR while dev
// invincibility is on. One place so ship_death + mine_update stay in agreement.
fn immune(ship: &Ship, dev: &Dev) -> bool {
    ship.invuln > 0.0 || dev.invincible
}

// Ship DIES on contact with an asteroid (unless invulnerable): burst + despawn,
// then either schedule a respawn or — on the last life — go to Game Over.
fn ship_death(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship)>,
    // Include the boss's rocks, but a rock still being REELED IN (grab in progress) is exempt — it
    // mustn't kill the player as the boss drags it across the field. A SETTLED shield rock (orbiting
    // the boss) still hurts, and free / thrown rocks always do (a thrown rock drops Shielded).
    asteroids: Query<(&Transform, &Asteroid, Option<&Shielded>)>,
) {
    if run.respawn > 0.0 {
        return; // already dead/respawning
    }
    for (e, t, ship) in &ships {
        if immune(ship, &dev) {
            continue;
        }
        let sp = t.translation.truncate();
        for (at, a, shielded) in &asteroids {
            if shielded.is_some_and(|sh| sh.grab < BOSS_GRAB_TIME) {
                continue; // mid-grab: harmless while it reels across the field
            }
            let rr = asteroid_radius(a.size) + SHIP_R * 0.6;
            if sp.distance_squared(at.translation.truncate()) < rr * rr {
                let mut rng = rand::thread_rng();
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, e, sp, &mut rng);
                break;
            }
        }
    }
}

fn respawn(mut commands: Commands, time: Res<Time>, mut run: ResMut<Run>, ships: Query<&Ship>) {
    if run.respawn <= 0.0 {
        return;
    }
    run.respawn -= time.delta_secs();
    if run.respawn <= 0.0 && ships.is_empty() {
        spawn_player(&mut commands);
    }
}

fn particle_update(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Particle)>,
) {
    let dt = time.delta_secs();
    for (e, mut t, mut p) in &mut q {
        t.translation.x += p.vel.x * dt;
        t.translation.y += p.vel.y * dt;
        p.vel *= 0.25_f32.powf(dt);
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn spin_asteroids(time: Res<Time>, mut q: Query<&mut Asteroid>) {
    let dt = time.delta_secs();
    for mut a in &mut q {
        a.rot += a.spin * dt;
    }
}

fn ship_bounds(arena: Res<Arena>, mut q: Query<(&mut Transform, &mut Velocity), With<Ship>>) {
    let h = arena.half;
    for (mut t, mut v) in &mut q {
        // no wrap — the edge is the bound; clamp position AND kill the into-wall
        // velocity so the ship can't push past the edge (which caused border ghosting)
        if t.translation.x < -h.x + SHIP_R {
            t.translation.x = -h.x + SHIP_R;
            v.0.x = v.0.x.max(0.0);
        } else if t.translation.x > h.x - SHIP_R {
            t.translation.x = h.x - SHIP_R;
            v.0.x = v.0.x.min(0.0);
        }
        if t.translation.y < -h.y + SHIP_R {
            t.translation.y = -h.y + SHIP_R;
            v.0.y = v.0.y.max(0.0);
        } else if t.translation.y > h.y - SHIP_R {
            t.translation.y = h.y - SHIP_R;
            v.0.y = v.0.y.min(0.0);
        }
    }
}

fn asteroid_bounds(arena: Res<Arena>, mut q: Query<(&mut Transform, &mut Velocity, &Asteroid), Without<Shielded>>) {
    let h = arena.half;
    let mut rng = rand::thread_rng();
    for (mut t, mut v, a) in &mut q {
        // never let a rock sit dead-still — elastic hits (or the boss shield) can zero
        // its velocity, which reads as "stuck". Keep a slow drift going.
        let sp = v.0.length();
        if sp < MIN_DRIFT {
            v.0 = if sp > 1.0 {
                v.0 / sp * MIN_DRIFT
            } else {
                Vec2::from_angle(rng.gen_range(0.0..TAU)) * MIN_DRIFT
            };
        }
        let r = asteroid_radius(a.size);
        let p = t.translation.truncate();
        if !(p.x < -h.x - r || p.x > h.x + r || p.y < -h.y - r || p.y > h.y + r) {
            continue;
        }
        let inward = rng.gen_range(50.0..130.0);
        let jitter = rng.gen_range(-40.0..40.0);
        match rng.gen_range(0..4) {
            0 => {
                t.translation = Vec3::new(-h.x - r, rng.gen_range(-h.y..h.y), 0.0);
                v.0 = Vec2::new(inward, jitter);
            }
            1 => {
                t.translation = Vec3::new(h.x + r, rng.gen_range(-h.y..h.y), 0.0);
                v.0 = Vec2::new(-inward, jitter);
            }
            2 => {
                t.translation = Vec3::new(rng.gen_range(-h.x..h.x), -h.y - r, 0.0);
                v.0 = Vec2::new(jitter, inward);
            }
            _ => {
                t.translation = Vec3::new(rng.gen_range(-h.x..h.x), h.y + r, 0.0);
                v.0 = Vec2::new(jitter, -inward);
            }
        }
    }
}

fn bullet_bounds(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut q: Query<(Entity, &Transform, &mut Bullet)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    for (e, t, mut b) in &mut q {
        b.life -= dt;
        let p = t.translation.truncate();
        if b.life <= 0.0 || p.x.abs() > h.x || p.y.abs() > h.y {
            commands.entity(e).despawn();
        }
    }
}

fn collisions(
    mut commands: Commands,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
    mut asteroids: Query<(Entity, &Transform, &mut Asteroid), (Without<Mine>, Without<Shielded>)>,
    mines: Query<(Entity, &Transform), With<Mine>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    mut shield_rocks: Query<(Entity, &Transform, &mut Asteroid), With<Shielded>>,
    mut bosses: Query<(&Transform, &mut Boss)>,
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
) {
    let mut rng = rand::thread_rng();
    let mut dead_b: HashSet<Entity> = HashSet::new();
    let mut dead_a: HashSet<Entity> = HashSet::new();
    let mut dead_m: HashSet<Entity> = HashSet::new();
    let mut dead_e: HashSet<Entity> = HashSet::new();
    let mut dead_s: HashSet<Entity> = HashSet::new();
    for (be, bt) in &bullets {
        if dead_b.contains(&be) {
            continue;
        }
        let bp = bt.translation.truncate();
        for (ae, at, mut a) in &mut asteroids {
            if dead_a.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(a.size) + BULLET_R;
            if bp.distance_squared(ap) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn(); // bullet is spent either way
                if a.hp > 1 {
                    a.hp -= 1; // dense rock cracks but holds — a chip, no split
                    burst(&mut commands, ap, dense_color(), 6, 160.0, &mut rng);
                } else {
                    dead_a.insert(ae);
                    break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, a.dense);
                    sfx.write(SoundFx::Break(a.size));
                }
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on an asteroid
        }
        for (me, mt) in &mines {
            if dead_m.contains(&me) {
                continue;
            }
            let rr = MINE_R + BULLET_R;
            if bp.distance_squared(mt.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                dead_m.insert(me);
                commands.entity(be).despawn();
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
                let mp = mt.translation.truncate();
                burst(&mut commands, mp, mine_color(), 24, 320.0, &mut rng);
                // shooting a mine detonates it: the blast shatters rocks in range
                // with fast chunks, same as any other detonation.
                blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut dead_a, mp);
                sfx.write(SoundFx::Mine);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a mine
        }
        for (ene, ent) in &enemies {
            if dead_e.contains(&ene) {
                continue;
            }
            let ep = ent.translation.truncate();
            let rr = ENEMY_R + BULLET_R;
            if bp.distance_squared(ep) < rr * rr {
                dead_b.insert(be);
                dead_e.insert(ene);
                commands.entity(be).despawn();
                kill_enemy(&mut commands, &mut score, &mut sfx, ene, ep, &mut rng); // dies in one shot
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on an enemy
        }
        // the boss's held shield rocks intercept shots — a hit shrinks the rock one
        // size IN PLACE (it stays on the arm); the smallest one shatters + frees the arm.
        for (se, st, mut sa) in &mut shield_rocks {
            if dead_s.contains(&se) {
                continue;
            }
            let sp = st.translation.truncate();
            let rr = asteroid_radius(sa.size) + BULLET_R;
            if bp.distance_squared(sp) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                burst(&mut commands, sp, rock_color(), 8, 200.0, &mut rng);
                if sa.size > 1 {
                    sa.size -= 1;
                    sa.verts = asteroid_verts(sa.size, &mut rng); // shrink, still held on the arm
                } else {
                    dead_s.insert(se);
                    commands.entity(se).despawn(); // smallest shatters, freeing the arm
                    score.0 += 20;
                }
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a shield rock
        }
        // the boss core takes a hit — bullets that slip through a gap in the spinning shield.
        for (bpos, mut boss) in &mut bosses {
            if boss.charge > 0.0 || boss.dying > 0.0 {
                continue; // invulnerable while charging up / already dying
            }
            let rr = BOSS_R + BULLET_R;
            if bp.distance_squared(bpos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                boss.hp -= 1;
                burst(&mut commands, bp, boss_color(), 6, 180.0, &mut rng);
                break;
            }
        }
    }
}

// 3-minute waves: survive the timer to advance; each new wave streams in more
// rocks (up to the cap). (Boss waves will end on kill instead — added in a later step.)
fn wave_timer(
    time: Res<Time>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut commands: Commands,
    arena: Res<Arena>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if wave.calm > 0.0 {
        wave.calm -= time.delta_secs(); // during the post-boss calm the timer is paused
        return;
    }
    if is_boss_wave(wave.level) {
        return; // a boss wave ends when the boss dies, not when the timer runs out
    }
    wave.timer -= time.delta_secs();
    if wave.timer > 0.0 {
        return;
    }
    wave.level += 1;
    wave.timer = WAVE_SECS;
    banner.timer = WAVE_BANNER_SECS; // flash the new wave number
    let target = population_target(wave.level);
    let have = asteroids.iter().count() as i32;
    let mut rng = rand::thread_rng();
    for _ in 0..(target - have).max(0) {
        let dense = roll_dense(wave.level, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, dense, false);
    }
}

// Stream replacement rocks in gradually so the field stays populated as you clear
// it — but NOT during the post-boss calm (kept clear for the reward).
fn top_up_asteroids(
    time: Res<Time>,
    mut clock: ResMut<SpawnClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    asteroids: Query<&Asteroid>,
) {
    if wave.calm > 0.0 {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let count = asteroids.iter().count() as i32;
    let bigs = asteroids.iter().filter(|a| a.size == 3).count() as i32;
    // refill toward the count target, AND separately keep big rocks above the floor even at the
    // cap — otherwise breaking large rocks leaves the field as nothing but small debris.
    if count < population_target(wave.level) || bigs < BIG_FLOOR {
        let mut rng = rand::thread_rng();
        let dense = roll_dense(wave.level, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, dense, bigs < BIG_FLOOR);
        clock.0 = SPAWN_INTERVAL;
    } else {
        clock.0 = 0.5; // at target — recheck shortly
    }
}

// The post-boss calm is a clean breather (and the pickup window): keep the field empty by
// despawning any leftover asteroids/mines — including the boss's scattered shield — for its whole
// duration. New spawns are already gated (top-ups bail while `calm > 0`).
fn clear_calm_field(wave: Res<Wave>, mut commands: Commands, junk: Query<Entity, Or<(With<Asteroid>, With<Mine>)>>) {
    if wave.calm > 0.0 {
        for e in &junk {
            commands.entity(e).despawn();
        }
    }
}

// A mine entering from a random edge.
fn spawn_edge_mine(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let inward = rng.gen_range(0.6..1.0) * MINE_SPEED;
    let jitter = rng.gen_range(-0.4..0.4) * MINE_SPEED;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - MINE_R, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + MINE_R, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - MINE_R), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + MINE_R), Vec2::new(jitter, -inward)),
    };
    commands.spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(vel), Transform::from_xyz(pos.x, pos.y, 0.0)));
}

// Stream mines in (wave 2+), capped as a fraction of the asteroids; not during calm.
fn top_up_mines(
    time: Res<Time>,
    mut clock: ResMut<MineClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    mines: Query<(), With<Mine>>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) {
        return; // no new mines during the calm or a boss wave
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let target = mine_target(wave.level, asteroids.iter().count() as i32);
    if (mines.iter().count() as i32) < target {
        let mut rng = rand::thread_rng();
        spawn_edge_mine(&mut commands, arena.half, &mut rng);
        clock.0 = MINE_SPAWN_INTERVAL;
    } else {
        clock.0 = 1.0;
    }
}

// Mines drift + recycle at the edges. Three ways one goes off, each blasting the
// rocks in range: it drifts into an asteroid (no life lost), the ship contacts it,
// or it's armed (ship was near) and the fuse elapses with the ship inside the blast.
fn mine_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut score: ResMut<Score>,
    dev: Res<Dev>,
    wave: Res<Wave>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<(Entity, &Transform, &Ship), Without<Mine>>,
    mut mines: Query<(Entity, &mut Transform, &mut Velocity, &mut Mine)>,
    // &mut to match blast_asteroids' type; only read here (iter + shared borrow)
    asteroids: Query<(Entity, &Transform, &mut Asteroid), (Without<Mine>, Without<Shielded>)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    let mut broken: HashSet<Entity> = HashSet::new();
    for (me, mut mt, mut mv, mut mine) in &mut mines {
        // recycle at the edges (reposition heading inward)
        let p = mt.translation.truncate();
        if p.x < -h.x - MINE_R || p.x > h.x + MINE_R || p.y < -h.y - MINE_R || p.y > h.y + MINE_R {
            if is_boss_wave(wave.level) {
                commands.entity(me).despawn(); // boss wave: mines drift off for good, no recycle
                continue;
            }
            let inward = rng.gen_range(0.6..1.0) * MINE_SPEED;
            let jitter = rng.gen_range(-0.4..0.4) * MINE_SPEED;
            match rng.gen_range(0..4) {
                0 => { mt.translation = Vec3::new(-h.x - MINE_R, rng.gen_range(-h.y..h.y), 0.0); mv.0 = Vec2::new(inward, jitter); }
                1 => { mt.translation = Vec3::new(h.x + MINE_R, rng.gen_range(-h.y..h.y), 0.0); mv.0 = Vec2::new(-inward, jitter); }
                2 => { mt.translation = Vec3::new(rng.gen_range(-h.x..h.x), -h.y - MINE_R, 0.0); mv.0 = Vec2::new(jitter, inward); }
                _ => { mt.translation = Vec3::new(rng.gen_range(-h.x..h.x), h.y + MINE_R, 0.0); mv.0 = Vec2::new(jitter, -inward); }
            }
            continue;
        }

        // A mine that has drifted into the field detonates the instant it touches a
        // rock — clearing it and its neighbours with fast chunks. No life is lost
        // here (that's only ship contact); this is the JS "asteroid-management" mine.
        let inside = p.x.abs() < h.x && p.y.abs() < h.y;
        if inside
            && asteroids.iter().any(|(_, at, a)| {
                let rr = MINE_R + asteroid_radius(a.size);
                p.distance_squared(at.translation.truncate()) < rr * rr
            })
        {
            burst(&mut commands, p, mine_color(), 26, 300.0, &mut rng);
            blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut broken, p);
            sfx.write(SoundFx::Mine);
            commands.entity(me).despawn();
            continue;
        }

        if run.respawn > 0.0 {
            continue; // ship already died this cycle
        }
        if let Some((se, st, sh)) = ship {
            if immune(sh, &dev) {
                continue;
            }
            let sp = st.translation.truncate();
            let d = p.distance(sp);
            if !mine.armed && d < MINE_TRIGGER_R {
                mine.armed = true;
                mine.fuse = MINE_FUSE;
            }
            if mine.armed {
                mine.fuse -= dt;
                let contact = d < MINE_R + SHIP_R;
                if contact || (mine.fuse <= 0.0 && d < MINE_BLAST_R) {
                    burst(&mut commands, p, mine_color(), 26, 300.0, &mut rng);
                    blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut broken, p);
                    sfx.write(SoundFx::Mine);
                    commands.entity(me).despawn();
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// Warp (Shift): fire a slow missile — on a very long cooldown — that tears open a
// black hole (see below). Ports the JS vortex/warp shot.
fn warp_fire(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut warp: ResMut<Warp>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<(&Ship, &Transform)>,
) {
    // While refilling (all charges were spent), tick the long cooldown; when it
    // ends, restore all charges. No firing during the refill.
    if warp.cooldown > 0.0 {
        warp.cooldown -= time.delta_secs();
        if warp.cooldown <= 0.0 {
            warp.cooldown = 0.0;
            warp.charges = WARP_MAX_CHARGES;
        }
        return;
    }
    let pressed = keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight);
    if !pressed || warp.charges <= 0 {
        return;
    }
    if let Some((ship, t)) = ships.iter().next() {
        let dir = Vec2::from_angle(ship.angle);
        let pos = t.translation.truncate() + dir * SHIP_R;
        commands.spawn((
            WarpMissile { life: WARP_MISSILE_LIFE },
            Velocity(dir * WARP_MISSILE_SPEED),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
        sfx.write(SoundFx::Warp);
        warp.charges -= 1;
        if warp.charges <= 0 {
            warp.cooldown = WARP_COOLDOWN; // last charge spent → start the long refill
        }
    }
}

// The missile flies for WARP_MISSILE_LIFE, then becomes a black hole in place — but it detonates
// early if it reaches the arena edge, and the hole is always clamped fully on-screen. Firing at
// the edge therefore opens a usable hole just inside the boundary instead of sailing off-screen
// where it could pull nothing in.
fn warp_missile_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut q: Query<(Entity, &Transform, &mut WarpMissile)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let margin = WARP_CONSUME_R; // keep the whole event horizon inside the arena
    for (e, t, mut m) in &mut q {
        m.life -= dt;
        let p = t.translation.truncate();
        let at_edge = p.x.abs() > h.x - margin || p.y.abs() > h.y - margin;
        if m.life <= 0.0 || at_edge {
            let c = Vec2::new(p.x.clamp(-h.x + margin, h.x - margin), p.y.clamp(-h.y + margin, h.y - margin));
            commands.entity(e).despawn();
            commands.spawn((BlackHole { life: WARP_HOLE_LIFE, spin: 0.0 }, Transform::from_xyz(c.x, c.y, 0.0)));
        }
    }
}

// ─────────────────────────────── enemy ships (wave 3+) ────────────────
fn enemy_target(level: i32, asteroids: i32) -> i32 {
    if !(ENEMY_FIRST_WAVE..=ENEMY_LAST_WAVE).contains(&level) {
        return 0; // no mobs before wave 3 or after wave 5
    }
    let raw = (level - ENEMY_FIRST_WAVE + 1) * ENEMY_PER_WAVE;
    raw.min((asteroids as f32 * ENEMY_MAX_FRACTION) as i32)
}

// An enemy gliding in from a random edge.
fn spawn_edge_enemy(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let inward = ENEMY_MAX_SPEED * 1.5;
    let jitter = rng.gen_range(-0.3..0.3) * ENEMY_MAX_SPEED;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - ENEMY_R, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + ENEMY_R, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - ENEMY_R), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + ENEMY_R), Vec2::new(jitter, -inward)),
    };
    commands.spawn((
        Enemy {
            fire: ENEMY_FIRE_EVERY + rng.gen_range(0.0..ENEMY_FIRE_JITTER),
            life: ENEMY_LIFETIME,
            strafe: if rng.gen_bool(0.5) { 1.0 } else { -1.0 },
            entered: false,
            fleeing: false,
        },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

// Stream enemies in (wave 3+), capped as a fraction of the asteroids; not during calm.
fn top_up_enemies(
    time: Res<Time>,
    mut clock: ResMut<EnemyClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    enemies: Query<(), With<Enemy>>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) {
        return; // no new enemies during the calm or a boss wave
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let target = enemy_target(wave.level, asteroids.iter().count() as i32);
    if (enemies.iter().count() as i32) < target {
        let mut rng = rand::thread_rng();
        spawn_edge_enemy(&mut commands, arena.half, &mut rng);
        clock.0 = ENEMY_SPAWN_INTERVAL;
    } else {
        clock.0 = 1.0;
    }
}

// Enemy movement + firing. `integrate` moves them (they carry a Velocity); here we
// only steer that velocity. Glide in, then hover + strafe around the ship, steering
// clear of mines and rocks and lobbing slow shots. A live warp overrides all of it
// (they get dragged in — handled in black_hole_update). After ENEMY_LIFETIME they
// flee the nearest edge and despawn, so they never overstay.
fn enemy_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<&Transform, With<Ship>>,
    mines: Query<&Transform, With<Mine>>,
    rocks: Query<(&Transform, &Asteroid)>,
    holes: Query<&Transform, With<BlackHole>>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity, &mut Enemy)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    // snapshot every enemy's position up front for mutual separation below — we can't
    // re-borrow the enemies query while iterating it mutably.
    let others: Vec<(Entity, Vec2)> = enemies.iter().map(|(e, t, _, _)| (e, t.translation.truncate())).collect();
    for (e, t, mut v, mut en) in &mut enemies {
        let p = t.translation.truncate();

        // caught in a warp → yield control (black_hole_update drags + consumes it)
        if holes.iter().any(|ht| ht.translation.truncate().distance(p) < WARP_PULL_RADIUS) {
            continue;
        }

        // glide in until fully on-screen, then settle
        if !en.entered {
            if p.x.abs() < h.x - ENEMY_R && p.y.abs() < h.y - ENEMY_R {
                en.entered = true;
                v.0 *= 0.4;
            }
            continue;
        }

        // lifetime → flee straight out and despawn once gone
        en.life -= dt;
        if en.life <= 0.0 {
            en.fleeing = true;
        }
        if en.fleeing {
            v.0 += p.normalize_or_zero() * ENEMY_ACCEL * dt;
            let cap = ENEMY_MAX_SPEED * 2.0;
            if v.0.length() > cap {
                v.0 = v.0.normalize() * cap;
            }
            if p.x.abs() > h.x + ENEMY_R * 3.0 || p.y.abs() > h.y + ENEMY_R * 3.0 {
                commands.entity(e).despawn();
            }
            continue;
        }

        // hover + strafe around the ship
        let mut acc = Vec2::ZERO;
        if let Some(sp) = ship {
            let to = sp - p;
            let d = to.length().max(1.0);
            let n = to / d;
            if d > ENEMY_PREF_DIST + 40.0 {
                acc += n;
            } else if d < ENEMY_PREF_DIST - 40.0 {
                acc -= n;
            }
            acc += Vec2::new(-n.y, n.x) * en.strafe * 0.6; // orbit
        }
        // avoid mines + rocks: push away from anything close (rock size widens reach)
        for mt in &mines {
            let away = p - mt.translation.truncate();
            let d = away.length();
            if d > 0.01 && d < ENEMY_AVOID_R {
                acc += away / d * (1.0 - d / ENEMY_AVOID_R) * 2.2;
            }
        }
        for (rt, a) in &rocks {
            let away = p - rt.translation.truncate();
            let d = away.length();
            let reach = ENEMY_AVOID_R + asteroid_radius(a.size);
            if d > 0.01 && d < reach {
                acc += away / d * (1.0 - d / reach) * 2.6;
            }
        }
        // keep clear of EACH OTHER — enemies spread into a loose formation, never a stack
        for &(oe, op) in &others {
            if oe == e {
                continue;
            }
            let away = p - op;
            let d = away.length();
            if d > 0.01 && d < ENEMY_SEP_R {
                acc += away / d * (1.0 - d / ENEMY_SEP_R) * 2.4;
            }
        }
        v.0 += acc * ENEMY_ACCEL * dt;
        v.0 *= 0.985_f32.powf(dt * 60.0); // damping (frame-rate independent)
        if v.0.length() > ENEMY_MAX_SPEED {
            v.0 = v.0.normalize() * ENEMY_MAX_SPEED;
        }
        // bounce off the arena edges so they stay in play
        if (p.x < -h.x + ENEMY_R && v.0.x < 0.0) || (p.x > h.x - ENEMY_R && v.0.x > 0.0) {
            v.0.x = -v.0.x;
        }
        if (p.y < -h.y + ENEMY_R && v.0.y < 0.0) || (p.y > h.y - ENEMY_R && v.0.y > 0.0) {
            v.0.y = -v.0.y;
        }

        // lob a slow shot at the ship
        en.fire -= dt;
        if en.fire <= 0.0 {
            en.fire = ENEMY_FIRE_EVERY + rng.gen_range(0.0..ENEMY_FIRE_JITTER);
            if let Some(sp) = ship {
                let dir = (sp - p).normalize_or_zero();
                if dir != Vec2::ZERO {
                    commands.spawn((
                        EnemyBullet { life: ENEMY_BULLET_LIFE },
                        Velocity(dir * ENEMY_BULLET_SPEED),
                        Transform::from_xyz(p.x, p.y, 0.0),
                    ));
                    sfx.write(SoundFx::EnemyShot);
                }
            }
        }
    }
}

// Enemy shots: `integrate` carries them; here we expire them (time / off-screen) and
// kill the ship on contact (respecting invuln + dev invincibility).
fn enemy_bullets(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut bullets: Query<(Entity, &Transform, &mut EnemyBullet)>,
    ships: Query<(Entity, &Transform, &Ship)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    for (be, bt, mut b) in &mut bullets {
        b.life -= dt;
        let p = bt.translation.truncate();
        if b.life <= 0.0 || p.x.abs() > h.x + 30.0 || p.y.abs() > h.y + 30.0 {
            commands.entity(be).despawn();
            continue;
        }
        if run.respawn > 0.0 {
            continue;
        }
        if let Some((se, st, sh)) = ship {
            if immune(sh, &dev) {
                continue;
            }
            let sp = st.translation.truncate();
            if p.distance(sp) < ENEMY_BULLET_R + SHIP_R {
                commands.entity(be).despawn();
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
            }
        }
    }
}

// ─────────────────────────────── octopus boss (waves 5, 10, …) ────────
// On entering a boss wave, spawn the boss (once) and clear the field of mines +
// enemy ships so only asteroids remain for it to grab.
fn boss_director(
    mut commands: Commands,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut state: ResMut<BossState>,
    mut enemies: Query<&mut Enemy>,
) {
    if !is_boss_wave(wave.level) || state.fought == wave.level {
        return;
    }
    state.fought = wave.level;
    commands.spawn((
        Boss {
            hp: BOSS_HP,
            rot: 0.0,
            pulse: 0.0,
            entered: false,
            charge: BOSS_CHARGE,
            fire: BOSS_FIRE_EVERY,
            capture: 0.4,
            dying: 0.0,
        },
        Transform::from_xyz(0.0, arena.half.y + BOSS_R + BOSS_ORBIT_R, 0.0),
    ));
    // Enemy ships bug out (flee). Existing mines are LEFT ALONE — they keep behaving
    // normally (drift/detonate/shootable) and drift off the edges (mine_update despawns
    // them at the edge during a boss wave instead of recycling). No new mines or enemies
    // spawn (top_up_mines / top_up_enemies are gated off on boss waves).
    for mut en in &mut enemies {
        en.fleeing = true;
    }
}

// Boss movement (glide in → bob near the top), charge-up (invulnerable), ship-contact
// kill, and death → big burst, release the shield, reward calm, advance the wave.
fn boss_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship), Without<Boss>>,
    mut bosses: Query<(Entity, &mut Transform, &mut Boss)>,
    mut shielded: Query<(Entity, &mut Velocity), With<Shielded>>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    for (be, mut bt, mut boss) in &mut bosses {
        boss.pulse += dt * 5.0;
        let p = bt.translation.truncate();

        // ── DYING: a slow death animation, then despawn → reward calm → advance ──
        if boss.dying > 0.0 {
            boss.dying -= dt;
            boss.rot += BOSS_SPIN * 2.5 * dt; // spins up as it comes apart
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..BOSS_R);
                burst(&mut commands, p + off, boss_color(), 3, 240.0, &mut rng); // crackle
            }
            if boss.dying <= 0.0 {
                burst(&mut commands, p, boss_color(), 50, 460.0, &mut rng); // final blast
                burst(&mut commands, p, Color::srgb(5.0, 4.0, 5.0), 24, 300.0, &mut rng);
                commands.entity(be).despawn();
                score.0 += BOSS_SCORE;
                // The chain-shot orb is offered ONLY after the FIRST boss (wave 5).
                // Grab it in the calm or lose it for good — skip = no chain shot ever.
                // (Checked before the level-up, so wave.level is still the boss wave.)
                if wave.level == BOSS_WAVE_INTERVAL {
                    let dir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                    commands.spawn((
                        Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE },
                        Velocity(dir * PICKUP_DRIFT),
                        Transform::from_xyz(0.0, 0.0, 0.0),
                    ));
                }
                wave.level += 1;
                wave.timer = WAVE_SECS;
                wave.calm = BOSS_CALM; // 10s calm — the pickup window
                banner.timer = WAVE_BANNER_SECS;
            }
            continue; // no movement / contact / damage while it dies
        }

        // ── core destroyed → begin dying: scatter the shield, then animate ──
        if boss.hp <= 0 {
            boss.dying = BOSS_DEATH_SECS;
            burst(&mut commands, p, boss_color(), 30, 320.0, &mut rng);
            for (se, mut sv) in &mut shielded {
                commands.entity(se).remove::<Shielded>();
                sv.0 = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(80.0..160.0);
            }
            continue;
        }

        // ── ALIVE: glide in → bob near the top, charge-up, ship-contact kill ──
        boss.rot += BOSS_SPIN * dt;
        let margin = BOSS_R + BOSS_ORBIT_R + 6.0;
        let mut p = p;
        let rest_y = h.y - margin;
        if !boss.entered {
            p.y -= BOSS_ENTER_SPEED * dt;
            if p.y <= rest_y {
                p.y = rest_y;
                boss.entered = true;
            }
        } else {
            if boss.charge > 0.0 {
                boss.charge -= dt;
            }
            // ROAM: ease toward a slow wandering target (a lazy Lissajous across the upper arena)
            // so the boss isn't a stationary target — you have to keep repositioning to hit its
            // core, and its shield sweeps the field. Wide horizontal sweep, gentle vertical dip.
            let cx = (boss.pulse * 0.16).sin() * (h.x - margin) * 0.72;
            let cy = rest_y - (h.y * 0.15) * (0.5 - 0.5 * (boss.pulse * 0.11).cos());
            let target = Vec2::new(cx, cy);
            let np = p + (target - p) * (1.0 - (-dt * 2.6).exp());
            p.x = np.x.clamp(-h.x + margin, h.x - margin);
            p.y = np.y.clamp(-h.y + margin, h.y - margin);
        }
        bt.translation = Vec3::new(p.x, p.y, 0.0);

        if run.respawn <= 0.0 {
            if let Some((se, st, sh)) = ship {
                let sp = st.translation.truncate();
                if !immune(sh, &dev) && p.distance(sp) < BOSS_R + SHIP_R {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// The shield: reel captured rocks into their rotating orbit slots, grab more field
// rocks to fill empty arms, and hurl any held rock whittled to its smallest size at
// the ship. Also ages out just-thrown rocks so they can't be re-grabbed instantly.
fn boss_shield(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    ships: Query<&Transform, (With<Ship>, Without<Boss>, Without<Asteroid>)>,
    mut bosses: Query<(&Transform, &mut Boss)>,
    mut shielded: Query<(Entity, &mut Transform, &mut Velocity, &Asteroid, &mut Shielded), Without<Boss>>,
    free: Query<(Entity, &Transform, &Asteroid), (Without<Shielded>, Without<Boss>, Without<Thrown>)>,
    mut thrown: Query<(Entity, &mut Thrown)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();

    for (te, mut th) in &mut thrown {
        th.0 -= dt;
        if th.0 <= 0.0 {
            commands.entity(te).remove::<Thrown>();
        }
    }

    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (bt, mut boss) in &mut bosses {
        if boss.hp <= 0 || !boss.entered {
            continue;
        }
        let bp = bt.translation.truncate();

        // hold: reel in / pin each shield rock to its rotating slot (mark used arms)
        let mut used = [false; BOSS_ARMS];
        for (_se, mut st, mut sv, _a, mut sh) in &mut shielded {
            if sh.slot < BOSS_ARMS {
                used[sh.slot] = true;
            }
            let ang = (sh.slot as f32 / BOSS_ARMS as f32) * TAU + boss.rot;
            let target = bp + Vec2::from_angle(ang) * BOSS_ORBIT_R;
            let cur = st.translation.truncate();
            let np = if sh.grab < BOSS_GRAB_TIME {
                sh.grab += dt;
                cur + (target - cur) * (1.0 - (-dt * 1.6).exp()) // slow, readable reel-in
            } else {
                target
            };
            st.translation = Vec3::new(np.x, np.y, 0.0);
            sv.0 = Vec2::ZERO;
        }

        // throw FIRST: fling a held smallest-size rock at the ship (frees an arm)…
        boss.fire -= dt;
        if boss.fire <= 0.0 {
            boss.fire = BOSS_FIRE_EVERY + rng.gen_range(0.0..BOSS_FIRE_JITTER);
            if let Some(sp) = ship {
                for (se, st, mut sv, a, sh) in &mut shielded {
                    if a.size == 1 {
                        let dir = (sp - st.translation.truncate()).normalize_or_zero();
                        if dir != Vec2::ZERO {
                            sv.0 = dir * BOSS_THROW_SPEED;
                            commands.entity(se).remove::<Shielded>();
                            commands.entity(se).insert(Thrown(2.0));
                            if sh.slot < BOSS_ARMS {
                                used[sh.slot] = false; // that arm is now free to refill
                            }
                        }
                        break;
                    }
                }
            }
        }

        // …THEN grab another rock into an empty arm, biggest first (better shield)
        boss.capture -= dt;
        if boss.capture <= 0.0 {
            boss.capture = BOSS_CAPTURE_EVERY;
            let held = used.iter().filter(|u| **u).count();
            if held < BOSS_ARMS {
                let mut best: Option<(Entity, u8, f32)> = None;
                for (fe, ft, fa) in &free {
                    let fp = ft.translation.truncate();
                    // only grab rocks that are ON-SCREEN and in the TOP half (where it lives) —
                    // no cross-screen yanks and nothing dragged in from off the edges
                    if fp.y <= 0.0 || fp.y >= h.y || fp.x.abs() >= h.x {
                        continue;
                    }
                    let d = fp.distance_squared(bp);
                    let better = best.is_none_or(|(_, bs, bd)| fa.size > bs || (fa.size == bs && d < bd));
                    if better {
                        best = Some((fe, fa.size, d));
                    }
                }
                if let Some((fe, _, _)) = best {
                    let slot = (0..BOSS_ARMS).find(|s| !used[*s]).unwrap_or(0);
                    commands.entity(fe).insert(Shielded { slot, grab: 0.0 });
                }
            }
        }
    }
}

// Free asteroids bounce off the boss's held shield rocks (treated as immovable), so
// they clatter around the shield instead of drifting straight through it.
fn shield_deflect(
    mut free: Query<(&mut Transform, &mut Velocity, &Asteroid), (Without<Shielded>, Without<Boss>)>,
    held: Query<(&Transform, &Asteroid), With<Shielded>>,
    bosses: Query<&Transform, With<Boss>>,
) {
    let boss_pos = bosses.iter().next().map(|t| t.translation.truncate());
    for (mut ft, mut fv, fa) in &mut free {
        let fr = asteroid_radius(fa.size);
        let mut fp = ft.translation.truncate();
        let mut hit = false;
        for (ht, ha) in &held {
            let hp = ht.translation.truncate();
            let delta = fp - hp;
            let d = delta.length();
            let min = fr + asteroid_radius(ha.size);
            if d < min && d > 0.01 {
                // Eject OUTWARD from the boss centre (not just away from the rock) so a
                // free rock can never get trapped inside the spinning shield ring — it's
                // always pushed out through it. Falls back to away-from-rock if needed.
                let n = match boss_pos {
                    Some(bp) if fp.distance(bp) > 1.0 => (fp - bp).normalize(),
                    _ => delta / d,
                };
                fp += n * (min - d);
                let vn = fv.0.dot(n);
                if vn < 0.0 {
                    fv.0 -= n * (2.0 * vn); // reflect so it heads back outward
                }
                hit = true;
            }
        }
        if hit {
            ft.translation.x = fp.x;
            ft.translation.y = fp.y;
        }
    }
}

// ─────────────────────────────── chain shot (secondary weapon) ────────
// Right-click fires a wide lightning BEAM — 3 charges that regenerate on a timer.
// Unlocked by the post-boss pickup. (Shift is the warp; primary fire is Space/LMB.)
fn chain_fire(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut chain: ResMut<Chain>,
    ships: Query<(&Ship, &Transform, &Velocity)>,
) {
    if !chain.unlocked {
        return;
    }
    let dt = time.delta_secs();
    if chain.cooldown > 0.0 {
        chain.cooldown -= dt;
    }
    if chain.charges < CHAIN_MAX_CHARGES {
        chain.recharge -= dt;
        if chain.recharge <= 0.0 {
            chain.charges += 1;
            chain.recharge = CHAIN_RECHARGE;
        }
    } else {
        chain.recharge = CHAIN_RECHARGE; // primed for the next spend
    }
    if !mouse.just_pressed(MouseButton::Right) || chain.charges <= 0 || chain.cooldown > 0.0 {
        return;
    }
    if let Some((ship, t, sv)) = ships.iter().next() {
        let dir = Vec2::from_angle(ship.angle);
        let pos = t.translation.truncate() + dir * SHIP_R;
        commands.spawn((
            ChainShot { life: CHAIN_LIFE, perp: Vec2::new(-dir.y, dir.x) },
            Velocity(dir * CHAIN_SPEED + sv.0 * 0.3),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
        chain.charges -= 1;
        chain.cooldown = CHAIN_COOLDOWN;
    }
}

// The beam travels (via `integrate`); here it expires and mows through everything its
// segment (centre ± perp·half) touches — rocks, enemies, mines, and the boss core.
fn chain_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
    mut chains: Query<(Entity, &Transform, &mut ChainShot)>,
    asteroids: Query<(Entity, &Transform, &Asteroid), (Without<Mine>, Without<Shielded>)>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    mines: Query<(Entity, &Transform), With<Mine>>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let mut dead: HashSet<Entity> = HashSet::new(); // consumed this frame
    for (ce, ct, mut cs) in &mut chains {
        cs.life -= dt;
        let c = ct.translation.truncate();
        if cs.life <= 0.0 || c.x.abs() > h.x + CHAIN_HALF || c.y.abs() > h.y + CHAIN_HALF {
            commands.entity(ce).despawn();
            continue;
        }
        let a = c + cs.perp * CHAIN_HALF;
        let b = c - cs.perp * CHAIN_HALF;
        for (ae, at, ast) in &asteroids {
            if dead.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(ast.size) + CHAIN_R;
            if seg_dist2(ap, a, b) < rr * rr {
                dead.insert(ae);
                // chain beam shears dense rocks outright — the beam ignores hp, like a mine
                break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, ast.size, 1.0, ast.dense);
                sfx.write(SoundFx::Break(ast.size));
            }
        }
        for (ee, et) in &enemies {
            if dead.contains(&ee) {
                continue;
            }
            let ep = et.translation.truncate();
            let rr = ENEMY_R + CHAIN_R;
            if seg_dist2(ep, a, b) < rr * rr {
                dead.insert(ee);
                kill_enemy(&mut commands, &mut score, &mut sfx, ee, ep, &mut rng);
            }
        }
        for (me, mt) in &mines {
            if dead.contains(&me) {
                continue;
            }
            let mp = mt.translation.truncate();
            let rr = MINE_R + CHAIN_R;
            if seg_dist2(mp, a, b) < rr * rr {
                dead.insert(me);
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
                burst(&mut commands, mp, mine_color(), 20, 320.0, &mut rng);
                sfx.write(SoundFx::Mine);
            }
        }
        // NOTE: the beam deliberately does NOT damage the boss core. Bosses are beaten
        // through their asteroid mechanic (the chain's job here is clearing the rocks),
        // so it can't be used to brute-force the core. See [[neon-edge-difficulty]].
    }
}

// The reward orb (post-boss calm): drifts + bounces on screen; fly into it to unlock
// the chain shot, or leave it (hardcore). It leaves when the calm window closes.
fn pickup_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut chain: ResMut<Chain>,
    ships: Query<&Transform, With<Ship>>,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
    mut pickups: Query<(Entity, &Transform, &mut Velocity, &mut Pickup)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (pe, pt, mut pv, mut pk) in &mut pickups {
        pk.life -= dt;
        if pk.life <= 0.0 {
            commands.entity(pe).despawn(); // window elapsed → gone for good (a single offer)
            continue;
        }
        pk.rot += dt * 1.8;
        pk.pulse += dt * 5.0;
        let p = pt.translation.truncate(); // integrate moves it; we just bounce + check grab
        if (p.x < -h.x + 60.0 && pv.0.x < 0.0) || (p.x > h.x - 60.0 && pv.0.x > 0.0) {
            pv.0.x = -pv.0.x;
        }
        if (p.y < -h.y + 60.0 && pv.0.y < 0.0) || (p.y > h.y - 60.0 && pv.0.y > 0.0) {
            pv.0.y = -pv.0.y;
        }
        // collect it by flying into it OR by shooting it
        let mut collected = ship.is_some_and(|sp| p.distance(sp) < PICKUP_R + SHIP_R);
        for (be, bt) in &bullets {
            if p.distance(bt.translation.truncate()) < PICKUP_R + BULLET_R {
                collected = true;
                commands.entity(be).despawn(); // the shot is spent grabbing it
                break;
            }
        }
        if collected {
            chain.unlocked = true;
            chain.charges = CHAIN_MAX_CHARGES;
            chain.recharge = CHAIN_RECHARGE;
            burst(&mut commands, p, chain_color(), 30, 300.0, &mut rng);
            commands.entity(pe).despawn();
        }
    }
}

// The black hole drags every nearby asteroid, enemy AND mine inward and consumes
// those that reach its core (the ship is immune — not in these queries).
fn black_hole_update(
    mut commands: Commands,
    time: Res<Time>,
    mut score: ResMut<Score>,
    mut holes: Query<(Entity, &Transform, &mut BlackHole)>,
    // boss-HELD rocks (Shielded) are exempt — you can't warp a boss's shield away; the
    // boss itself carries neither Asteroid nor Enemy, so it's exempt automatically.
    mut asteroids: Query<(Entity, &Transform, &mut Velocity, &Asteroid), Without<Shielded>>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity), (With<Enemy>, Without<Asteroid>)>,
    mut mines: Query<(Entity, &Transform, &mut Velocity), (With<Mine>, Without<Asteroid>, Without<Enemy>)>,
) {
    let dt = time.delta_secs();
    let pull_r = WARP_PULL_RADIUS;
    let mut rng = rand::thread_rng();
    for (he, ht, mut hole) in &mut holes {
        hole.spin += dt * 3.5;
        hole.life -= dt;
        let hp = ht.translation.truncate();
        // consume on EDGE contact (center within horizon + own radius) so a big rock is
        // eaten the instant it touches the hole — it never survives to clump at the mouth.
        for (ae, at, mut av, a) in &mut asteroids {
            let ap = at.translation.truncate();
            if ap.distance(hp) < WARP_CONSUME_R + asteroid_radius(a.size) {
                score.0 += 30;
                burst(&mut commands, ap, rock_color(), 14, 280.0, &mut rng);
                commands.entity(ae).despawn();
            } else {
                av.0 += warp_pull(ap, hp, pull_r, dt);
            }
        }
        // enemies get sucked in and consumed just like rocks
        for (ee, et, mut ev) in &mut enemies {
            let ep = et.translation.truncate();
            if ep.distance(hp) < WARP_CONSUME_R + ENEMY_R {
                score.0 += ENEMY_SCORE;
                burst(&mut commands, ep, enemy_color(), 18, 300.0, &mut rng);
                commands.entity(ee).despawn();
            } else {
                ev.0 += warp_pull(ep, hp, pull_r, dt);
            }
        }
        // mines are dragged in and consumed too
        for (me, mt, mut mv) in &mut mines {
            let mp = mt.translation.truncate();
            if mp.distance(hp) < WARP_CONSUME_R + MINE_R {
                score.0 += MINE_SCORE;
                burst(&mut commands, mp, mine_color(), 16, 300.0, &mut rng);
                commands.entity(me).despawn();
            } else {
                mv.0 += warp_pull(mp, hp, pull_r, dt);
            }
        }
        // suck-in sparks streaking toward the core (extra juice)
        for _ in 0..2 {
            let a = rng.gen_range(0.0..TAU);
            let sp = hp + Vec2::from_angle(a) * rng.gen_range(70.0..150.0);
            let inv = (hp - sp).normalize_or_zero() * rng.gen_range(220.0..360.0);
            commands.spawn((
                Particle { vel: inv, life: 0.3, ttl: 0.3, color: warp_color() },
                Transform::from_xyz(sp.x, sp.y, 0.0),
            ));
        }
        if hole.life <= 0.0 {
            commands.entity(he).despawn();
        }
    }
}

// Drive the grid warp: ease toward 1 while a hole is open (pulling the grid in),
// then rubber-snap 1→0 (overshooting negative) once it closes.
fn update_warp_field(time: Res<Time>, mut wf: ResMut<WarpField>, holes: Query<&Transform, With<BlackHole>>) {
    let dt = time.delta_secs();
    if let Some(ht) = holes.iter().next() {
        wf.pos = ht.translation.truncate();
        wf.active = true;
        wf.snap_t = 0.0;
        wf.amount = (wf.amount + dt * 3.5).min(1.0);
    } else {
        if wf.active {
            wf.active = false;
            wf.snap_t = 0.0;
        }
        if wf.amount != 0.0 {
            wf.snap_t += dt;
            let k = (wf.snap_t / WARP_SNAP_DUR).min(1.0);
            wf.amount = 1.0 - ease_out_elastic(k);
            if k >= 1.0 {
                wf.amount = 0.0;
            }
        }
    }
}

// ─────────────────────────────── always-on systems ────────────────────
fn update_arena(mut arena: ResMut<Arena>, windows: Query<&Window>) {
    if let Some(w) = windows.iter().next() {
        arena.half = Vec2::new(w.width() * 0.5, w.height() * 0.5);
    }
}

fn update_wave_text(wave: Res<Wave>, mut q: Query<&mut Text, With<WaveText>>) {
    let secs = wave.timer.max(0.0) as i32;
    for mut t in &mut q {
        t.0 = if is_boss_wave(wave.level) {
            format!("WAVE {}    BOSS", wave.level)
        } else {
            format!("WAVE {}    {}:{:02}", wave.level, secs / 60, secs % 60)
        };
    }
}

fn update_score_text(score: Res<Score>, mut q: Query<&mut Text, With<ScoreText>>) {
    for mut t in &mut q {
        t.0 = format!("SCORE {}", score.0);
    }
}

// The big "WAVE n" flash: quick fade-in, hold, then fade out over WAVE_BANNER_FADE.
fn wave_banner_update(
    time: Res<Time>,
    wave: Res<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut q: Query<(&mut Text, &mut TextColor), With<WaveBannerText>>,
) {
    if banner.timer > 0.0 {
        banner.timer -= time.delta_secs();
    }
    let t = banner.timer.max(0.0);
    let fade_in = ((WAVE_BANNER_SECS - t) / 0.2).clamp(0.0, 1.0);
    let fade_out = (t / WAVE_BANNER_FADE).clamp(0.0, 1.0);
    let alpha = fade_in.min(fade_out);
    for (mut text, mut color) in &mut q {
        if alpha > 0.0 {
            text.0 = format!("WAVE {}", wave.level);
        }
        color.0 = color.0.with_alpha(alpha);
    }
}

fn render(
    mut gizmos: Gizmos,
    time: Res<Time>,
    arena: Res<Arena>,
    run: Res<Run>,
    dev: Res<Dev>,
    // warp + chain paired into one tuple param to stay within Bevy's 16-param system limit
    abilities: (Res<Warp>, Res<Chain>),
    wf: Res<WarpField>,
    stars: Query<(&Star, &Transform)>,
    ships: Query<(&Ship, &Transform)>,
    asteroids: Query<(&Asteroid, &Transform)>,
    bullets: Query<(&Bullet, &Transform)>,
    particles: Query<(&Particle, &Transform)>,
    holes: Query<(&BlackHole, &Transform)>,
    missiles: Query<&Transform, With<WarpMissile>>,
    mines_q: Query<(&Mine, &Transform)>,
    // grouped into one tuple param to stay within Bevy's 16-param system limit
    foes: (Query<(&Enemy, &Transform)>, Query<&Transform, With<EnemyBullet>>),
) {
    let h = arena.half;
    let t = time.elapsed_secs();
    let (warp_res, chain) = (&abilities.0, &abilities.1);

    // stars (backmost)
    let star = star_color();
    for (s, st) in &stars {
        let tw = 0.35 + 0.65 * (t * 1.6 + s.phase).sin().max(0.0);
        let c = st.translation.truncate();
        let col = dim(star, s.bright * tw);
        gizmos.line_2d(c - Vec2::X * 1.3, c + Vec2::X * 1.3, col);
        gizmos.line_2d(c - Vec2::Y * 1.3, c + Vec2::Y * 1.3, col);
    }

    // grid — faint, brighter per-line shimmer; bends toward an active warp hole
    // (and rubber-snaps back afterward). Straight 2-point lines unless warping.
    let grid = grid_color();
    let warping = wf.amount.abs() > 0.001;
    const SUBDIV: usize = 14;
    let mut i = 0;
    let mut x = -(h.x / GRID_CELL).floor() * GRID_CELL;
    while x <= h.x {
        let sh = 0.5 + 1.1 * (0.5 + 0.5 * (i as f32 * 0.7 + t * 1.2).sin());
        let col = dim(grid, sh);
        if warping {
            let pts: Vec<Vec2> = (0..=SUBDIV)
                .map(|s| warp_point(Vec2::new(x, -h.y + 2.0 * h.y * (s as f32 / SUBDIV as f32)), &wf))
                .collect();
            gizmos.linestrip_2d(pts, col);
        } else {
            gizmos.line_2d(Vec2::new(x, -h.y), Vec2::new(x, h.y), col);
        }
        x += GRID_CELL;
        i += 1;
    }
    let mut j = 0;
    let mut y = -(h.y / GRID_CELL).floor() * GRID_CELL;
    while y <= h.y {
        let sh = 0.5 + 1.1 * (0.5 + 0.5 * (j as f32 * 0.7 + t * 1.2 + 1.7).sin());
        let col = dim(grid, sh);
        if warping {
            let pts: Vec<Vec2> = (0..=SUBDIV)
                .map(|s| warp_point(Vec2::new(-h.x + 2.0 * h.x * (s as f32 / SUBDIV as f32), y), &wf))
                .collect();
            gizmos.linestrip_2d(pts, col);
        } else {
            gizmos.line_2d(Vec2::new(-h.x, y), Vec2::new(h.x, y), col);
        }
        y += GRID_CELL;
        j += 1;
    }

    // asteroids — dense (green) rocks carry a concentric inner ring that shrinks as
    // they're chipped, so their tanky state reads at a glance.
    let rock = rock_color();
    let dense = dense_color();
    for (a, at) in &asteroids {
        let c = at.translation.truncate();
        let rot = Vec2::from_angle(a.rot);
        let col = if a.dense { dense } else { rock };
        let ring = |scale: f32| {
            let mut pts: Vec<Vec2> = a.verts.iter().map(|v| c + rot.rotate(*v * scale)).collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            pts
        };
        gizmos.linestrip_2d(ring(1.0), col);
        if a.dense {
            let frac = a.hp.max(1) as f32 / a.size.max(1) as f32; // full shell → shrinks to a small core
            gizmos.linestrip_2d(ring(0.35 + 0.3 * frac), col);
        }
    }

    // mines — crimson diamonds; blink faster once armed (the ship is near)
    let mc = mine_color();
    for (mine, mt) in &mines_q {
        let c = mt.translation.truncate();
        if !mine.armed || ((t * 12.0) as i32) % 2 == 0 {
            let r = MINE_R;
            let pts = [
                c + Vec2::new(0.0, r),
                c + Vec2::new(r, 0.0),
                c + Vec2::new(0.0, -r),
                c + Vec2::new(-r, 0.0),
                c + Vec2::new(0.0, r),
            ];
            gizmos.linestrip_2d(pts, mc);
            gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.4, mc);
        }
    }

    // enemy ships — neon-yellow orbs with a pulsing core (dim while fleeing out)
    let ec = enemy_color();
    for (en, et) in &foes.0 {
        let c = et.translation.truncate();
        let throb = 1.0 + 0.1 * (t * 6.0 + en.life).sin();
        let body = if en.fleeing { dim(ec, 0.55) } else { ec };
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_R * throb, body);
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_R * 0.45 * throb, body);
    }
    // enemy shots — yellow dots with a white-hot core
    for et in &foes.1 {
        let c = et.translation.truncate();
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_BULLET_R, ec);
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_BULLET_R * 0.5, Color::srgb(5.0, 5.0, 4.0));
    }

    // warp: a big black-hole DRAIN spiral (streams corkscrew inward, like water
    // down a drain) with layered glow + comet heads + a pulsing core.
    // The warp shot glows harder than the rest of the scene via brighter HDR colors
    // (NOT more global bloom, which would light up everything else too).
    let glow = 3.3; // the vortex glows much harder than the rest of the scene (brighter bloom)
    let warp = dim(warp_color(), glow);
    let comet = dim(Color::srgb(3.6, 2.2, 5.2), glow); // stream comet heads
    let corec = dim(Color::srgb(4.0, 2.6, 5.2), glow); // pulsing hot core
    let arms = 7;
    let segs = 14;
    let r_out = 112.0; // arms stay INSIDE the event-horizon ring — no spiral spilling past it
    let r_in = 22.0; // fatter throat/core — a bigger, hungrier center
    let wind = 2.4; // looser wrap — arms spiral in as spokes, don't close into rings
    for (hole, ht) in &holes {
        let c = ht.translation.truncate();
        let f = (hole.life / WARP_HOLE_LIFE).clamp(0.0, 1.0);
        let pulse = 1.0 + 0.12 * (hole.spin * 2.0).sin();
        // funnel arms — a clean spiral drawn segment-by-segment, fading to nothing at the rim
        // and brightening toward the core. Contained inside the event-horizon ring, so there's
        // no separate outer circle — it reads as a drain converging inward.
        for a in 0..arms {
            let a0 = a as f32 / arms as f32 * TAU;
            let pt = |p: f32| {
                let rad = r_out - (r_out - r_in) * p;
                c + Vec2::from_angle(a0 + wind * p + hole.spin) * rad
            };
            for k in 0..segs {
                let p0 = k as f32 / segs as f32;
                let p1 = (k + 1) as f32 / segs as f32;
                // p1: 0 at the (near-invisible) outer rim → 1 at the bright inner throat
                gizmos.line_2d(pt(p0), pt(p1), dim(warp, 0.46 * f * (0.05 + 0.95 * p1)));
            }
        }
        // bright streams travelling INWARD (comet: tail streak + bright head), brightening and
        // growing as they fall toward the core (tiny and dim at the rim → no dots on any circle)
        for a in 0..arms {
            let a0 = a as f32 / arms as f32 * TAU;
            let hp = (hole.spin * 0.18 + a as f32 / arms as f32).rem_euclid(1.0);
            let tp = (hp - 0.16).max(0.0);
            let head = c + Vec2::from_angle(a0 + wind * hp + hole.spin) * (r_out - (r_out - r_in) * hp);
            let tail = c + Vec2::from_angle(a0 + wind * tp + hole.spin) * (r_out - (r_out - r_in) * tp);
            let b = f * hp * hp; // brightens sharply as it accelerates inward (dark at the rim)
            gizmos.line_2d(tail, head, dim(warp, 0.9 * b));
            gizmos.circle_2d(Isometry2d::from_translation(head), 1.5 + 2.0 * hp, dim(comet, b));
        }
        // EVENT HORIZON — the kill boundary (anything crossing WARP_CONSUME_R is devoured) and
        // now the vortex's clean outer edge: a single bright pulsing rim, nothing beyond it.
        gizmos.circle_2d(Isometry2d::from_translation(c), WARP_CONSUME_R * pulse, dim(warp, 0.75 * f));
        // bright rim + pulsing hot core + a white-hot center for a searing bloom
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 30.0 * f) * pulse, dim(warp, 0.9 * f));
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 14.0 * f) * pulse, dim(corec, f));
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 6.0 * f) * pulse, dim(Color::srgb(6.0, 5.0, 6.0), f));
    }
    for mt in &missiles {
        gizmos.circle_2d(Isometry2d::from_translation(mt.translation.truncate()), 6.0, warp);
    }

    // particles
    for (p, pt) in &particles {
        let f = (p.life / p.ttl).clamp(0.0, 1.0);
        let c = pt.translation.truncate();
        let dir = p.vel.normalize_or_zero();
        gizmos.line_2d(c, c - dir * 6.0, dim(p.color, f));
    }

    // bullets — a small bright head trailing a tapering purple flame. The flame
    // blobs shrink to a fine point at the tail and heat up toward the head (deep
    // purple tip → hot lavender base); the head itself is kept compact.
    let core = Color::srgb(5.0, 4.2, 5.6); // white-hot center
    let flame_tip = dim(bullet_color(), 0.5); // deep purple (tail)
    let flame_base = mix(bullet_color(), core, 0.35); // hot lavender (near the head)
    for (b, bt) in &bullets {
        let c = bt.translation.truncate();
        let n = b.trail.len();
        for k in 0..n {
            let f = if n > 1 { k as f32 / (n - 1) as f32 } else { 1.0 }; // 0 tail → 1 head
            let r = BULLET_R * (0.12 + 0.85 * f); // taper to a point at the tail
            gizmos.circle_2d(Isometry2d::from_translation(b.trail[k]), r, mix(flame_tip, flame_base, f * f));
        }
        gizmos.circle_2d(Isometry2d::from_translation(c), BULLET_R * 0.75, flame_base);
        gizmos.circle_2d(Isometry2d::from_translation(c), BULLET_R * 0.38, core);
    }

    // ship — flame + hull (blinks while invulnerable)
    let sc = ship_color();
    for (s, st) in &ships {
        let c = st.translation.truncate();
        // DEV invincibility: a steady shield ring so it's obvious god-mode is on.
        // Drawn before the blink skip so it stays visible through respawn flicker.
        if dev.invincible {
            let pulse = 1.0 + 0.06 * (t * 4.0).sin();
            gizmos.circle_2d(Isometry2d::from_translation(c), SHIP_R * 2.2 * pulse, dim(sc, 0.6));
        }
        if s.invuln > 0.0 && (s.invuln * 12.0) as i32 % 2 == 0 {
            continue;
        }
        let rot = Vec2::from_angle(s.angle);
        if s.flame > 0.02 {
            let f = s.flame * (0.6 + 0.4 * (t * 40.0).sin().abs());
            let flame = [
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, -5.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5 - 17.0 * f, 0.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, 5.0)),
            ];
            gizmos.linestrip_2d(flame, dim(flame_color(), f));
        }
        let hull = [
            Vec2::new(SHIP_R, 0.0),
            Vec2::new(-SHIP_R * 0.7, -SHIP_R * 0.7),
            Vec2::new(-SHIP_R * 0.4, 0.0),
            Vec2::new(-SHIP_R * 0.7, SHIP_R * 0.7),
            Vec2::new(SHIP_R, 0.0),
        ];
        let pts: Vec<Vec2> = hull.iter().map(|v| c + rot.rotate(*v)).collect();
        gizmos.linestrip_2d(pts, sc);
    }

    // lives HUD icons (top-right, under the "LIVES" label)
    for k in 0..run.lives.max(0) {
        let p = Vec2::new(h.x - 32.0 - k as f32 * 24.0, h.y - 48.0);
        let icon = [
            p + Vec2::new(0.0, 9.0),
            p + Vec2::new(-7.0, -7.0),
            p + Vec2::new(0.0, -3.0),
            p + Vec2::new(7.0, -7.0),
            p + Vec2::new(0.0, 9.0),
        ];
        gizmos.linestrip_2d(icon, sc);
    }

    // warp charge pips (bottom-center): lit per available charge + a refill bar
    let gap = 22.0;
    let py = -h.y + 28.0;
    for k in 0..WARP_MAX_CHARGES {
        let px = (k as f32 - (WARP_MAX_CHARGES as f32 - 1.0) * 0.5) * gap;
        let col = if k < warp_res.charges { warp } else { dim(warp, 0.14) };
        gizmos.circle_2d(Isometry2d::from_translation(Vec2::new(px, py)), 5.0, col);
    }
    if warp_res.cooldown > 0.0 {
        let prog = 1.0 - warp_res.cooldown / WARP_COOLDOWN;
        let w = gap * (WARP_MAX_CHARGES as f32 - 1.0);
        gizmos.line_2d(Vec2::new(-w * 0.5, py - 11.0), Vec2::new(-w * 0.5 + w * prog, py - 11.0), dim(warp, 0.7));
    }

    // chain-shot charges (bottom-left) — shown only once the beam is unlocked. A little
    // bolt glyph + electric-violet pips + a refill bar toward the next charge; mirrors warp.
    if chain.unlocked {
        let cc = chain_color();
        let bx = -h.x + 30.0;
        let bolt = [
            Vec2::new(bx + 2.0, py + 8.0),
            Vec2::new(bx - 3.0, py + 1.0),
            Vec2::new(bx + 1.0, py + 1.0),
            Vec2::new(bx - 2.0, py - 8.0),
        ];
        gizmos.linestrip_2d(bolt.to_vec(), cc);
        let x0 = bx + 22.0;
        for k in 0..CHAIN_MAX_CHARGES {
            let px = x0 + k as f32 * gap;
            let col = if k < chain.charges { cc } else { dim(cc, 0.14) };
            gizmos.circle_2d(Isometry2d::from_translation(Vec2::new(px, py)), 5.0, col);
        }
        if chain.charges < CHAIN_MAX_CHARGES {
            let prog = 1.0 - chain.recharge / CHAIN_RECHARGE;
            let w = gap * (CHAIN_MAX_CHARGES as f32 - 1.0);
            gizmos.line_2d(Vec2::new(x0, py - 11.0), Vec2::new(x0 + w * prog, py - 11.0), dim(cc, 0.7));
        }
    }
}

// Boss rendering, split out of `render` (it was at the 16-param system limit): the
// background cameo telegraph, the magenta core (a jagged pulsing star, blinking while
// it charges), and the octopus arms — curved tapering tentacles to each shield rock.
// The shield rocks themselves draw as normal asteroids in `render`.
fn render_boss(
    mut gizmos: Gizmos,
    time: Res<Time>,
    arena: Res<Arena>,
    wave: Res<Wave>,
    bosses: Query<(&Boss, &Transform)>,
    shielded: Query<(&Transform, &Shielded)>,
) {
    let h = arena.half;
    let t = time.elapsed_secs();
    let mc = boss_color();

    // cameo: the boss drifts by in the background in the run-up to its wave
    if !is_boss_wave(wave.level) && is_boss_wave(wave.level + 1) && wave.calm <= 0.0 && wave.timer <= BOSS_CAMEO_SECS {
        let prog = ((BOSS_CAMEO_SECS - wave.timer) / BOSS_CAMEO_SECS).clamp(0.0, 1.0);
        let c = Vec2::new(-h.x - 150.0 + (2.0 * h.x + 300.0) * prog, h.y * 0.45);
        let ghost: Vec<Vec2> = (0..=20)
            .map(|k| {
                let a = k as f32 / 20.0 * TAU + t * 0.3;
                let r = BOSS_R * 1.5 * if k % 2 == 0 { 1.0 } else { 0.72 };
                c + Vec2::from_angle(a) * r
            })
            .collect();
        gizmos.linestrip_2d(ghost, dim(mc, 0.22));
    }

    for (boss, bt) in &bosses {
        let c = bt.translation.truncate();
        // arms: a curved, tapering tentacle to each shield rock
        for (st, sh) in &shielded {
            let a = st.translation.truncate();
            let d = a - c;
            let dist = d.length().max(1.0);
            let perp = Vec2::new(-d.y, d.x) / dist;
            let curl = (boss.pulse * 1.4 + sh.slot as f32 * 1.7).sin() * dist * 0.22;
            let mid = c + d * 0.5 + perp * curl;
            let n = 9;
            let pts: Vec<Vec2> = (0..=n)
                .map(|i| {
                    let tt = i as f32 / n as f32;
                    let it = 1.0 - tt;
                    c * (it * it) + mid * (2.0 * it * tt) + a * (tt * tt) // quadratic bezier
                })
                .collect();
            gizmos.linestrip_2d(pts, dim(mc, 0.7));
        }
        // core: jagged pulsing star + glowing center. Blinks while charging (invuln);
        // while DYING it shrinks toward nothing and flickers as it comes apart.
        let throb = 1.0 + 0.1 * boss.pulse.sin();
        let scale = if boss.dying > 0.0 { (boss.dying / BOSS_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let blink = boss.charge > 0.0 || boss.dying > 0.0;
        if !blink || ((boss.pulse * 3.0) as i32) % 2 == 0 {
            let star: Vec<Vec2> = (0..=20)
                .map(|k| {
                    let a = k as f32 / 20.0 * TAU + boss.rot;
                    let r = BOSS_R * throb * scale * if k % 2 == 0 { 1.0 } else { 0.72 };
                    c + Vec2::from_angle(a) * r
                })
                .collect();
            gizmos.linestrip_2d(star, mc);
            gizmos.circle_2d(Isometry2d::from_translation(c), BOSS_R * 0.4 * throb * scale, mc);
        }
        // HP bar (top-center): a dim full-width track with a bright magenta fill.
        // Hidden once it's dying (the fight's over).
        if boss.dying <= 0.0 {
            let frac = (boss.hp as f32 / BOSS_HP as f32).clamp(0.0, 1.0);
            let bw = 380.0;
            let x0 = -bw / 2.0;
            let by = h.y - 42.0;
            for i in 0..6 {
                let yy = by + (i as f32 - 2.5) * 2.2;
                gizmos.line_2d(Vec2::new(x0, yy), Vec2::new(x0 + bw, yy), dim(mc, 0.18)); // track
                gizmos.line_2d(Vec2::new(x0, yy), Vec2::new(x0 + bw * frac, yy), mc); // fill
            }
        }
    }
}

// Chain beams + the reward pickup orb (split out of `render` for the 16-param limit).
fn render_extras(
    mut gizmos: Gizmos,
    time: Res<Time>,
    chains: Query<(&Transform, &ChainShot)>,
    pickups: Query<(&Transform, &Pickup)>,
) {
    let t = time.elapsed_secs();
    let cc = chain_color();
    let white = Color::srgb(5.0, 4.6, 5.6);
    // chain beams — a jagged lightning bolt between the two ends + bright end dots
    for (ct, cs) in &chains {
        let c = ct.translation.truncate();
        let a = c + cs.perp * CHAIN_HALF;
        let b = c - cs.perp * CHAIN_HALF;
        let segs = 7;
        let along = b - a;
        let perp = cs.perp.perp(); // unit perpendicular to the beam (i.e. the travel axis)
        let pts: Vec<Vec2> = (0..=segs)
            .map(|i| {
                let f = i as f32 / segs as f32;
                let jag = if i == 0 || i == segs { 0.0 } else { (t * 45.0 + i as f32 * 2.3).sin() * 12.0 };
                a + along * f + perp * jag
            })
            .collect();
        gizmos.linestrip_2d(pts, cc);
        gizmos.circle_2d(Isometry2d::from_translation(a), 4.0, white);
        gizmos.circle_2d(Isometry2d::from_translation(b), 4.0, white);
    }
    // reward orb — a pulsing hexagon with a bright core
    for (pt, pk) in &pickups {
        let c = pt.translation.truncate();
        let throb = 1.0 + 0.14 * pk.pulse.sin();
        let hex: Vec<Vec2> = (0..=6)
            .map(|i| c + Vec2::from_angle(i as f32 / 6.0 * TAU + pk.rot) * PICKUP_R * throb)
            .collect();
        gizmos.linestrip_2d(hex, cc);
        gizmos.circle_2d(Isometry2d::from_translation(c), PICKUP_R * 0.3 * throb, white);
    }
}

// ─────────────────────────────── pause / game-over ────────────────────
fn pause_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    match state.get() {
        GameState::Playing => next.set(GameState::Paused),
        GameState::Paused => next.set(GameState::Playing),
        GameState::GameOver => {}
    }
}

// A full-screen centered overlay root. Returns EntityCommands so the caller adds children.
fn overlay(commands: &mut Commands, marker: impl Component) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.01, 0.06, 0.72)),
        ))
        .id()
}

fn text(font_size: f32, color: Color, s: &str) -> (Text, TextFont, TextColor) {
    (Text::new(s), TextFont { font_size, ..default() }, TextColor(color))
}

fn spawn_pause_ui(mut commands: Commands) {
    let root = overlay(&mut commands, PauseUi);
    commands.entity(root).with_children(|p| {
        p.spawn(text(56.0, Color::srgb(2.4, 1.0, 4.6), "PAUSED"));
        p.spawn(text(22.0, Color::srgb(0.7, 0.85, 1.2), "Esc  —  Resume"));
    });
}

fn spawn_gameover_ui(mut commands: Commands, score: Res<Score>) {
    let root = overlay(&mut commands, GameOverUi);
    let score_line = format!("SCORE   {}", score.0);
    commands.entity(root).with_children(|p| {
        p.spawn(text(64.0, Color::srgb(5.0, 1.2, 1.2), "GAME OVER"));
        p.spawn(text(26.0, Color::srgb(0.85, 0.9, 1.2), &score_line));
        p.spawn(text(22.0, Color::srgb(0.7, 0.85, 1.2), "Enter  —  Restart"));
    });
}

fn despawn_pause_ui(mut commands: Commands, q: Query<Entity, With<PauseUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn despawn_gameover_ui(mut commands: Commands, q: Query<Entity, With<GameOverUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// Restart the run from the Game-Over screen (Enter): clear the field, reset,
// spawn a fresh ship + asteroids, back to Playing.
fn gameover_restart(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    mut run: ResMut<Run>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut warp: ResMut<Warp>,
    mut progress: (ResMut<BossState>, ResMut<Chain>), // bundled (16-param limit)
    ships: Query<Entity, With<Ship>>,
    asteroids: Query<Entity, With<Asteroid>>,
    bullets: Query<Entity, With<Bullet>>,
    particles: Query<Entity, With<Particle>>,
    holes: Query<Entity, With<BlackHole>>,
    missiles: Query<Entity, With<WarpMissile>>,
    // mines + enemies + enemy shots + boss + chain beams + pickup, one tuple param
    // (16-param limit). Shield/thrown rocks are Asteroids, so `asteroids` clears them.
    hazards: (
        Query<Entity, With<Mine>>,
        Query<Entity, With<Enemy>>,
        Query<Entity, With<EnemyBullet>>,
        Query<Entity, With<Boss>>,
        Query<Entity, With<ChainShot>>,
        Query<Entity, With<Pickup>>,
    ),
) {
    if !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space)) {
        return;
    }
    for e in ships
        .iter()
        .chain(&asteroids)
        .chain(&bullets)
        .chain(&particles)
        .chain(&holes)
        .chain(&missiles)
        .chain(&hazards.0)
        .chain(&hazards.1)
        .chain(&hazards.2)
        .chain(&hazards.3)
        .chain(&hazards.4)
        .chain(&hazards.5)
    {
        commands.entity(e).despawn();
    }
    run.lives = START_LIVES;
    run.respawn = 0.0;
    score.0 = 0;
    wave.level = 1;
    wave.timer = WAVE_SECS;
    wave.calm = 0.0;
    progress.0.fought = 0; // so the next boss wave spawns a fresh boss
    *progress.1 = Chain::default(); // must re-earn the chain shot
    banner.timer = WAVE_BANNER_SECS; // re-flash "WAVE 1"
    warp.charges = WARP_MAX_CHARGES;
    warp.cooldown = 0.0;
    spawn_player(&mut commands);
    next.set(GameState::Playing); // field refills from the edges via top_up_asteroids
}

// ─────────────────────────────── music ────────────────────────────────
#[derive(Component)]
struct Music;

const MUSIC_VOLUME: f32 = 0.55;

// What the soundtrack should be playing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MusicCue {
    Main,    // the full-length main track (loops)
    Buildup, // the ~10 s riser in the run-up to a boss (one-shot)
    Boss,    // the boss track (loops)
    Silence, // the post-boss calm — a deliberate breather, no music
}

// The soundtrack director. Normal play loops the main track; the last 10 s before a boss play a
// buildup riser; boss waves loop the boss track; the post-boss calm is silent.
#[derive(Resource)]
struct MusicDirector {
    main: Handle<AudioSource>,
    boss: Handle<AudioSource>,
    buildup: Handle<AudioSource>,
    cue: Option<MusicCue>, // what's live (None = nothing spawned yet)
    muted: bool,
}

// Synthesize the tracks up front and install the director. The first cue is spawned by
// `music_director` on its first run.
fn start_music(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let main = sources.add(AudioSource { bytes: audio::main_track_wav().into() });
    let boss = sources.add(AudioSource { bytes: audio::boss_track_wav().into() });
    let buildup = sources.add(AudioSource { bytes: audio::boss_buildup_wav().into() });
    commands.insert_resource(MusicDirector { main, boss, buildup, cue: None, muted: false });
}

// Spawn a Music player. Loops for the main/boss tracks; one-shot (Despawn) for the buildup riser.
fn play_track(commands: &mut Commands, handle: Handle<AudioSource>, muted: bool, looping: bool) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: if looping { PlaybackMode::Loop } else { PlaybackMode::Despawn },
            volume: Volume::Linear(if muted { 0.0 } else { MUSIC_VOLUME }),
            ..default()
        },
        Music,
    ));
}

// Pick the right cue for the current moment and swap to it when it changes; `M` mutes.
fn music_director(
    keys: Res<ButtonInput<KeyCode>>,
    wave: Res<Wave>,
    mut dir: ResMut<MusicDirector>,
    mut commands: Commands,
    music: Query<Entity, With<Music>>,
    mut sinks: Query<&mut AudioSink, With<Music>>,
) {
    // M — mute/unmute by volume
    if keys.just_pressed(KeyCode::KeyM) {
        dir.muted = !dir.muted;
        let v = Volume::Linear(if dir.muted { 0.0 } else { MUSIC_VOLUME });
        for mut sink in &mut sinks {
            sink.set_volume(v);
        }
    }

    let desired = if wave.calm > 0.0 {
        MusicCue::Silence // post-boss breather — let it be quiet, don't slam the track back on
    } else if is_boss_wave(wave.level) {
        MusicCue::Boss
    } else if is_boss_wave(wave.level + 1) && wave.timer <= BOSS_CAMEO_SECS {
        MusicCue::Buildup // last 10 s before the boss wave → riser leads in
    } else {
        MusicCue::Main
    };

    if dir.cue != Some(desired) {
        for e in &music {
            commands.entity(e).despawn(); // matches by marker, fires even before the sink exists
        }
        match desired {
            MusicCue::Silence => {}
            MusicCue::Main => {
                let h = dir.main.clone();
                play_track(&mut commands, h, dir.muted, true);
            }
            MusicCue::Boss => {
                let h = dir.boss.clone();
                play_track(&mut commands, h, dir.muted, true);
            }
            MusicCue::Buildup => {
                let h = dir.buildup.clone();
                play_track(&mut commands, h, dir.muted, false);
            }
        }
        dir.cue = Some(desired);
    }
}

// ─────────────────────────────── sound effects ────────────────────────
// One event per SFX; gameplay systems fire them, `play_sfx` turns them into one-shot
// sounds — deduped to at most one of each kind per frame so a mine blast / chain sweep
// hitting many rocks doesn't stack into a wall of noise.
#[derive(Event, Clone, Copy)]
enum SoundFx {
    Fire,
    Break(u8), // asteroid size (1..3) → picks a deeper clip for bigger rocks
    Mine,
    Death,     // the player ship being destroyed
    EnemyShot, // an enemy mob firing
    EnemyDie,  // an enemy mob destroyed
    Warp,      // the warp/black-hole launch
}

// Pre-synthesized SFX clips (see `audio.rs`), built once at startup.
#[derive(Resource)]
struct SfxBank {
    fire: Handle<AudioSource>,
    break_rock: [Handle<AudioSource>; 3], // indexed by size-1: [small, mid, large] — big = deeper
    mine: Handle<AudioSource>,
    death: Handle<AudioSource>,
    enemy_shot: Handle<AudioSource>,
    enemy_die: Handle<AudioSource>,
    warp: Handle<AudioSource>,
}

fn start_sfx(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    commands.insert_resource(SfxBank {
        fire: sources.add(AudioSource { bytes: audio::fire_sfx_wav().into() }),
        break_rock: [1u8, 2, 3].map(|s| sources.add(AudioSource { bytes: audio::break_sfx_wav(s).into() })),
        mine: sources.add(AudioSource { bytes: audio::mine_sfx_wav().into() }),
        death: sources.add(AudioSource { bytes: audio::death_sfx_wav().into() }),
        enemy_shot: sources.add(AudioSource { bytes: audio::enemy_shot_wav().into() }),
        enemy_die: sources.add(AudioSource { bytes: audio::enemy_die_wav().into() }),
        warp: sources.add(AudioSource { bytes: audio::warp_wav().into() }),
    });
}

// Spawn a one-shot sound that despawns itself when it finishes.
fn one_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            ..default()
        },
    ));
}

fn play_sfx(mut commands: Commands, bank: Option<Res<SfxBank>>, mut events: EventReader<SoundFx>) {
    let Some(bank) = bank else {
        events.clear();
        return;
    };
    let (mut fire, mut mine, mut death, mut eshot, mut edie, mut warp) =
        (false, false, false, false, false, false);
    let mut brk: Option<u8> = None; // deepest (largest) rock that broke this frame
    for e in events.read() {
        match e {
            SoundFx::Fire => fire = true,
            SoundFx::Break(sz) => brk = Some(brk.unwrap_or(0).max(*sz)),
            SoundFx::Mine => mine = true,
            SoundFx::Death => death = true,
            SoundFx::EnemyShot => eshot = true,
            SoundFx::EnemyDie => edie = true,
            SoundFx::Warp => warp = true,
        }
    }
    if fire {
        one_shot(&mut commands, bank.fire.clone(), 0.3);
    }
    if let Some(sz) = brk {
        // one break sound per frame (the biggest rock's), kept well under the music (0.55) —
        // breaks are constant, so they mustn't dominate
        let clip = bank.break_rock[(sz.clamp(1, 3) - 1) as usize].clone();
        one_shot(&mut commands, clip, 0.3);
    }
    if mine {
        one_shot(&mut commands, bank.mine.clone(), 0.8); // the explosion should dominate briefly
    }
    if death {
        one_shot(&mut commands, bank.death.clone(), 0.7); // losing a life is a big, clear event
    }
    if eshot {
        one_shot(&mut commands, bank.enemy_shot.clone(), 0.28); // incoming fire — audible, not naggy
    }
    if edie {
        one_shot(&mut commands, bank.enemy_die.clone(), 0.45);
    }
    if warp {
        one_shot(&mut commands, bank.warp.clone(), 0.6); // the ultimate — a big, distinct whoosh
    }
}

// ─────────────────────────────── app ──────────────────────────────────
// DEV: F1 toggles invincibility. Compiled into debug builds ONLY — a release build
// has no system that can flip `Dev`, so god-mode can't ship by accident.
#[cfg(debug_assertions)]
fn dev_toggle(keys: Res<ButtonInput<KeyCode>>, mut dev: ResMut<Dev>) {
    if keys.just_pressed(KeyCode::F1) {
        dev.invincible = !dev.invincible;
        info!("DEV invincibility: {}", if dev.invincible { "ON" } else { "OFF" });
    }
}

// DEV: F2 skips to the next wave — kills the boss on a boss wave (so it advances via
// the normal death path), otherwise just expires the timer. Debug builds only.
#[cfg(debug_assertions)]
fn dev_wave_skip(keys: Res<ButtonInput<KeyCode>>, mut wave: ResMut<Wave>, mut bosses: Query<&mut Boss>) {
    if keys.just_pressed(KeyCode::F2) {
        if let Some(mut b) = bosses.iter_mut().next() {
            b.hp = 0;
        } else {
            wave.timer = 0.0;
        }
        info!("DEV skip → next wave");
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VIOLET EDGE".into(),
                resolution: (1280.0_f32, 800.0_f32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.02, 0.01, 0.06)))
        .insert_resource(Score(0))
        .insert_resource(Run { lives: START_LIVES, respawn: 0.0 })
        .insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 })
        .insert_resource(WaveBanner { timer: WAVE_BANNER_SECS }) // flash "WAVE 1" at start
        .insert_resource(SpawnClock::default())
        .insert_resource(MineClock::default())
        .insert_resource(EnemyClock::default())
        .insert_resource(Warp { charges: WARP_MAX_CHARGES, cooldown: 0.0 })
        .insert_resource(WarpField::default())
        .insert_resource(Arena { half: Vec2::new(640.0, 400.0) })
        .insert_resource(Dev::default())
        .insert_resource(BossState::default())
        .insert_resource(Chain::default())
        .add_event::<SoundFx>()
        .init_state::<GameState>()
        .add_systems(Startup, (setup, spawn_hud, start_music, start_sfx))
        // always: keep the arena sized, handle pause input, refresh the HUD text
        .add_systems(Update, (update_arena, pause_toggle, update_wave_text, update_score_text, wave_banner_update).chain())
        // render in PostUpdate so it ALWAYS runs after every Update system (incl.
        // ship_bounds) — draws final positions, no border ghosting; runs in all states
        .add_systems(PostUpdate, (render, render_boss, render_extras))
        // gameplay only while Playing
        .add_systems(
            Update,
            // split into THREE chained groups (Bevy's tuple limit is 20 systems);
            // the groups still run fully in order, first → second → third.
            (
                (
                    ship_control,
                    fire,
                    chain_fire,
                    warp_fire,
                    integrate,
                    bullet_trail,
                    warp_missile_update,
                    black_hole_update,
                    update_warp_field,
                    asteroid_collisions,
                )
                    .chain(),
                (
                    ship_death,
                    mine_update,
                    enemy_update,
                    enemy_bullets,
                    boss_director,
                    boss_update,
                    boss_shield,
                    shield_deflect,
                    chain_update,
                    pickup_update,
                    respawn,
                )
                    .chain(),
                (
                    particle_update,
                    spin_asteroids,
                    ship_bounds,
                    asteroid_bounds,
                    bullet_bounds,
                    collisions,
                    wave_timer,
                    top_up_asteroids,
                    top_up_mines,
                    top_up_enemies,
                    clear_calm_field,
                )
                    .chain(),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(Update, (music_director, play_sfx))
        .add_systems(Update, gameover_restart.run_if(in_state(GameState::GameOver)))
        .add_systems(OnEnter(GameState::Paused), spawn_pause_ui)
        .add_systems(OnExit(GameState::Paused), despawn_pause_ui)
        .add_systems(OnEnter(GameState::GameOver), spawn_gameover_ui)
        .add_systems(OnExit(GameState::GameOver), despawn_gameover_ui);
    // dev-only tools (F1 invincibility, F2 wave-skip); compiled out of release builds
    #[cfg(debug_assertions)]
    app.add_systems(Update, (dev_toggle, dev_wave_skip));
    app.run();
}

// ─────────────────────────────── headless tests ───────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_spawns_a_bullet() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Space);
        app.insert_resource(input);
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.world_mut().spawn((
            Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, (ship_control, fire, integrate).chain());
        app.update();
        let n = app.world_mut().query::<&Bullet>().iter(app.world()).count();
        assert!(n > 0, "pressing Space should spawn a bullet, got {n}");
    }

    #[test]
    fn bullet_destroys_overlapping_asteroid() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new() },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        assert!(app.world().resource::<Score>().0 >= 20, "a bullet on an asteroid should score a hit");
    }

    #[test]
    fn dense_rock_chips_before_it_breaks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a dense size-2 rock = 2 hp: the first hit only cracks it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: true, hp: 2 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        // still one rock, now at 1 hp, and nothing scored — a chip, not a break
        let rocks: Vec<(bool, i32)> = app.world_mut().query::<&Asteroid>().iter(app.world()).map(|a| (a.dense, a.hp)).collect();
        assert_eq!(rocks.len(), 1, "the dense rock survives the first hit");
        assert_eq!(rocks[0], (true, 1), "the first hit chips hp from 2 to 1");
        assert_eq!(app.world().resource::<Score>().0, 0, "a chip scores nothing");

        // a second bullet finishes it: it shatters into two dense chunks and scores double
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.update();
        let chunks: Vec<bool> = app.world_mut().query::<&Asteroid>().iter(app.world()).map(|a| a.dense).collect();
        assert_eq!(chunks.len(), 2, "the second hit shatters it into two chunks");
        assert!(chunks.iter().all(|&d| d), "dense chunks inherit the density");
        assert!(app.world().resource::<Score>().0 >= 100, "a dense size-2 break scores double (>=100)");
    }

    #[test]
    fn split_chunks_fly_apart_not_stack() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        // a size-2 rock with a bullet sitting on it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new() },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let chunks: Vec<(Vec2, Vec2)> = app
            .world_mut()
            .query::<(&Transform, &Velocity, &Asteroid)>()
            .iter(app.world())
            .map(|(t, v, _)| (t.translation.truncate(), v.0))
            .collect();
        assert_eq!(chunks.len(), 2, "a size-2 rock splits into two chunks");
        // they must spawn clear of each other, not stacked at the break point
        let sep = chunks[0].0.distance(chunks[1].0);
        assert!(sep > asteroid_radius(1) * 2.0, "chunks must spawn clear of each other, got separation {sep}");
        // both must actually be launched, in opposing directions (fly apart)
        assert!(chunks[0].1.length() > 1.0 && chunks[1].1.length() > 1.0, "both chunks need a launch velocity");
        assert!(chunks[0].1.dot(chunks[1].1) < 0.0, "chunks should head in opposing directions");
    }

    #[test]
    fn overlapping_asteroids_push_apart() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        let a = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(10.0, 0.0)),
                Transform::from_xyz(-10.0, 0.0, 0.0),
            ))
            .id();
        let b = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(-10.0, 0.0)),
                Transform::from_xyz(10.0, 0.0, 0.0),
            ))
            .id();
        app.add_systems(Update, asteroid_collisions);
        for _ in 0..40 {
            app.update();
        }
        let pa = app.world().entity(a).get::<Transform>().unwrap().translation.truncate();
        let pb = app.world().entity(b).get::<Transform>().unwrap().translation.truncate();
        assert!(pa.distance(pb) >= 129.0, "overlapping asteroids should separate; got {}", pa.distance(pb));
    }

    #[test]
    fn a_stopped_rock_keeps_drifting() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a rock at rest, mid-arena (elastic hits could have zeroed it → "stuck")
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, asteroid_bounds);
        app.update();
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!((v.length() - MIN_DRIFT).abs() < 1.0, "a stopped rock is nudged back to a slow drift, got {}", v.length());
    }

    #[test]
    fn ship_dies_on_asteroid_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 0, "ship should die on contact");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life should be lost");
        assert!(app.world().resource::<Run>().respawn > 0.0, "a respawn should be scheduled");
    }

    #[test]
    fn rock_reeling_in_does_not_kill_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a rock still reeling in (grab in progress), sitting on the ship, must NOT cost a life
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 0.0 },
        ));
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "the ship survives a rock the boss is dragging in");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life is lost mid-grab");
    }

    #[test]
    fn settled_shield_rock_still_kills_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a rock that has FINISHED reeling in (settled into orbit) is a live hazard again
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: BOSS_GRAB_TIME },
        ));
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 0, "flying into the orbiting shield still kills");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost to a settled shield rock");
    }

    #[test]
    fn ship_death_emits_a_sound() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let sounds: Vec<SoundFx> = app.world_mut().resource_mut::<Events<SoundFx>>().drain().collect();
        assert!(sounds.iter().any(|&s| matches!(s, SoundFx::Death)), "destroying the ship should emit a death sound");
    }

    #[test]
    fn invulnerable_ship_survives_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 2.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 1, "an invulnerable ship should NOT die");
    }

    #[test]
    fn dev_invincibility_prevents_death() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev { invincible: true }); // god-mode ON
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 1, "dev invincibility should keep the ship alive through a lethal hit");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life lost while invincible");
    }

    #[test]
    fn last_life_ends_the_run() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 1, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        // last life → no respawn scheduled (it goes to Game Over instead)
        assert_eq!(app.world().resource::<Run>().lives, 0);
        assert_eq!(app.world().resource::<Run>().respawn, 0.0, "the last life is game over, not a respawn");
    }

    #[test]
    fn wave_advances_when_timer_expires() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Wave { level: 1, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, wave_timer);
        app.update();
        assert_eq!(app.world().resource::<Wave>().level, 2, "expiring the timer should advance the wave");
    }

    #[test]
    fn black_hole_consumes_nearby_asteroid() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(10.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(n, 0, "an asteroid within the consume radius should be eaten");
        assert!(app.world().resource::<Score>().0 >= 30, "consuming should score");
    }

    #[test]
    fn warp_spends_three_charges_then_starts_cooldown() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Warp { charges: WARP_MAX_CHARGES, cooldown: 0.0 });
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ShiftLeft); // stays "just_pressed" (no clear system) → fires each frame
        app.insert_resource(input);
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, warp_fire);
        for _ in 0..5 {
            app.update();
        }
        let warp = app.world().resource::<Warp>();
        assert_eq!(warp.charges, 0, "all three charges should be spent");
        assert!(warp.cooldown > 0.0, "spending the last charge starts the long cooldown");
        let missiles = app.world_mut().query::<&WarpMissile>().iter(app.world()).count();
        assert_eq!(missiles, 3, "exactly three warp missiles should have fired");
    }

    #[test]
    fn top_up_streams_rocks_when_below_target() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, top_up_asteroids);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert!(n >= 1, "an empty field below target should stream in a replacement rock");
    }

    #[test]
    fn no_top_up_during_calm() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 5.0 }); // in the post-boss calm
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, top_up_asteroids);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(n, 0, "no rocks should spawn during the post-boss calm");
    }

    #[test]
    fn mine_target_gates_and_caps() {
        assert_eq!(mine_target(1, 10), 0, "no mines before wave 2");
        assert_eq!(mine_target(2, 10), 2, "wave 2: (2-2+1)*2 = 2, under the 50%-of-10 cap");
        assert_eq!(mine_target(5, 4), 2, "capped at 50% of 4 asteroids");
    }

    #[test]
    fn armed_mine_kills_ship_on_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: true, fuse: 0.0 }, // armed, overlapping the ship
            Velocity(Vec2::ZERO),
            Transform::from_xyz(5.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(ships, 0, "an armed mine should kill the ship on contact");
        assert_eq!(mines, 0, "the mine detonates and despawns");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost");
    }

    #[test]
    fn mine_drifting_into_a_rock_detonates_and_shatters_it() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a size-2 rock and a mine overlapping it, mid-field, with NO ship present
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(rocks, 2, "the mine should shatter the size-2 rock into two size-1 chunks, got {rocks}");
        assert_eq!(mines, 0, "the mine detonates on contact with the rock and despawns");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life is lost when a mine hits a rock");
    }

    #[test]
    fn mines_drift_off_during_a_boss_wave() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 5, timer: WAVE_SECS, calm: 0.0 }); // boss wave
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a mine that has drifted just off the right edge
        app.world_mut().spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::new(50.0, 0.0)), Transform::from_xyz(700.0, 0.0, 0.0)));
        app.add_systems(Update, mine_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "off-edge mines drift off (despawn) during a boss wave instead of recycling");
    }

    #[test]
    fn enemy_target_gates_and_caps() {
        assert_eq!(enemy_target(2, 100), 0, "no enemies before wave 3");
        assert_eq!(enemy_target(3, 100), 2, "wave 3 → 2, well under the cap");
        assert_eq!(enemy_target(5, 6), 1, "capped to a fraction of the rock count");
        assert_eq!(enemy_target(6, 100), 0, "yellow mobs stop after wave 5");
    }

    #[test]
    fn bullet_kills_enemy_in_one_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new() },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "one bullet should destroy the enemy");
        assert_eq!(app.world().resource::<Score>().0, ENEMY_SCORE, "killing an enemy scores");
    }

    #[test]
    fn enemy_bullet_kills_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            EnemyBullet { life: 2.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, enemy_bullets);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 0, "an enemy shot on the ship kills it");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost");
    }

    #[test]
    fn warp_consumes_enemy() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "an enemy at the core is consumed by the warp");
        assert_eq!(app.world().resource::<Score>().0, ENEMY_SCORE, "consuming an enemy scores");
    }

    #[test]
    fn lingering_enemy_flees_and_despawns() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // entered, out of life, already past the far edge → the flee branch despawns it
        app.world_mut().spawn((
            Enemy { fire: 5.0, life: 0.0, strafe: 1.0, entered: true, fleeing: true },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(900.0, 0.0, 0.0),
        ));
        app.add_systems(Update, enemy_update);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "an enemy that has fled off-screen despawns");
    }

    #[test]
    fn boss_wave_detection() {
        assert!(!is_boss_wave(4));
        assert!(is_boss_wave(5));
        assert!(is_boss_wave(10));
        assert!(!is_boss_wave(6));
    }

    #[test]
    fn boss_spawns_and_enemies_flee_on_boss_wave() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Wave { level: 5, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        let mine = app
            .world_mut()
            .spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let enemy = app
            .world_mut()
            .spawn((Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 1, "a boss spawns");
        assert!(app.world().entity(mine).get::<Mine>().is_some(), "mines are NOT wiped — they linger + behave normally");
        assert!(app.world().entity(enemy).get::<Enemy>().unwrap().fleeing, "enemy ships just leave (flee)");
        assert_eq!(app.world().resource::<BossState>().fought, 5);
    }

    #[test]
    fn boss_hp_zero_begins_slow_death() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 5, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        app.insert_resource(Chain::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: 0, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 1.0, capture: 1.0, dying: 0.0 }, Transform::from_xyz(0.0, 200.0, 0.0)));
        app.add_systems(Update, boss_update);
        app.update();
        // hp<=0 BEGINS the slow death — the boss lingers (dying), wave not yet advanced
        // (the despawn + calm + level-up fire once the death timer elapses, ~2.2s later).
        let mut q = app.world_mut().query::<&Boss>();
        let dying: Vec<f32> = q.iter(app.world()).map(|b| b.dying).collect();
        assert_eq!(dying.len(), 1, "the boss lingers through its death animation");
        assert!(dying[0] > 0.0, "hp<=0 starts the death timer instead of an instant despawn");
        assert_eq!(app.world().resource::<Wave>().level, 5, "the wave advances only when the death finishes");
    }

    #[test]
    fn boss_captures_a_free_rock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(100.0, 100.0, 0.0)))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(rock).get::<Shielded>().is_some(), "the boss grabs a nearby top-half rock onto its shield");
    }

    #[test]
    fn bullet_damages_the_boss_core() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 5.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let hp = app.world_mut().query::<&Boss>().iter(app.world()).next().unwrap().hp;
        assert_eq!(hp, BOSS_HP - 1, "a bullet through a gap chips the core");
    }

    #[test]
    fn boss_throws_a_smallest_shield_rock_at_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, -200.0, 0.0)));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 0.0, capture: 5.0, dying: 0.0 }, Transform::from_xyz(0.0, 200.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 20.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 250.0, 0.0),
                Shielded { slot: 0, grab: 1.0 },
            ))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(rock).get::<Shielded>().is_none(), "the size-1 rock is released");
        assert!(app.world().entity(rock).get::<Thrown>().is_some(), "and flagged as just-thrown");
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!(v.length() > 1.0 && v.y < 0.0, "flung toward the ship (which is below it)");
    }

    #[test]
    fn warp_consumes_a_mine() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, black_hole_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "a mine at the core is consumed by the warp");
    }

    #[test]
    fn warp_pulls_a_distant_mine() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a mine 250px out — inside WARP_PULL_RADIUS (360), so it should be tugged inward
        let mine = app
            .world_mut()
            .spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(250.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, black_hole_update);
        for _ in 0..5 {
            app.update(); // first frame's dt is 0; a few frames give the pull real time
        }
        let v = app.world().entity(mine).get::<Velocity>().unwrap().0;
        assert!(v.x < 0.0, "the mine is pulled back toward the hole (leftward), got {v:?}");
    }

    #[test]
    fn warp_spares_boss_held_rocks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a boss-HELD rock sitting right on the hole — it must survive (can't warp a shield away)
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Shielded { slot: 0, grab: 1.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a FREE rock just as close — it must be devoured
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Shielded>>().iter(app.world()).count(), 1, "the boss-held rock is exempt");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "only the held rock is left — the free one was devoured");
    }

    #[test]
    fn boss_grabs_the_biggest_rock_first() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a small rock CLOSE and a large rock FAR — it should still take the large one
        let small = app
            .world_mut()
            .spawn((Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(60.0, 80.0, 0.0)))
            .id();
        let large = app
            .world_mut()
            .spawn((Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(300.0, 80.0, 0.0)))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(large).get::<Shielded>().is_some(), "the big rock is grabbed");
        assert!(app.world().entity(small).get::<Shielded>().is_none(), "the near small rock is passed over");
    }

    #[test]
    fn free_rock_bounces_off_a_shield_rock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        // a held (shield) rock at the origin
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 1.0 },
        ));
        // a free rock overlapping it, drifting further in (+x… toward the shield centre is -x)
        let free = app
            .world_mut()
            .spawn((Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::new(-50.0, 0.0)), Transform::from_xyz(40.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, shield_deflect);
        app.update();
        let ft = app.world().entity(free).get::<Transform>().unwrap().translation.truncate();
        let fv = app.world().entity(free).get::<Velocity>().unwrap().0;
        assert!(ft.length() >= asteroid_radius(3) + asteroid_radius(1) - 0.5, "the free rock is pushed clear of the shield rock");
        assert!(fv.x > 0.0, "its inward velocity is reflected back outward");
    }

    #[test]
    fn shooting_a_shield_rock_shrinks_it_in_place() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
                Shielded { slot: 0, grab: 1.0 },
            ))
            .id();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world().entity(rock).get::<Asteroid>().unwrap().size, 2, "a shot shield rock drops one size…");
        assert!(app.world().entity(rock).get::<Shielded>().is_some(), "…but stays on the arm");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "no free chunks are spawned");
    }

    #[test]
    fn shooting_the_smallest_shield_rock_frees_the_arm() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 1.0 },
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the smallest shield rock shatters when shot");
    }

    #[test]
    fn pickup_grants_the_chain_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 6, timer: WAVE_SECS, calm: 5.0 }); // calm window open
        app.insert_resource(Chain::default());
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert!(app.world().resource::<Chain>().unlocked, "flying into the orb unlocks the chain shot");
        assert_eq!(app.world().resource::<Chain>().charges, CHAIN_MAX_CHARGES, "and fills its charges");
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "the orb is consumed");
    }

    #[test]
    fn ungrabbed_pickup_expires_after_its_life() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        // life already elapsed → the orb leaves for good (a single, missable offer)
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(200.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "an ungrabbed orb leaves once its life elapses");
        assert!(!app.world().resource::<Chain>().unlocked, "…and the chain shot stays locked");
    }

    #[test]
    fn shooting_the_pickup_grants_the_chain_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        // no ship — a bullet overlapping the orb should grab it on its own
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new() }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert!(app.world().resource::<Chain>().unlocked, "shooting the orb unlocks the chain shot");
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "the orb is consumed");
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "the shot that grabbed it is spent");
    }

    #[test]
    fn music_cues_follow_the_boss_cycle() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(MusicDirector {
            main: Handle::default(),
            boss: Handle::default(),
            buildup: Handle::default(),
            cue: None,
            muted: false,
        });
        app.add_systems(Update, music_director);

        // normal play → the main track
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main), "normal play uses the main track");
        assert_eq!(app.world_mut().query::<&Music>().iter(app.world()).count(), 1, "one track is live");

        // last 10 s before the boss (wave 4, timer low) → the buildup riser
        {
            let mut w = app.world_mut().resource_mut::<Wave>();
            w.level = 4;
            w.timer = 5.0;
        }
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Buildup), "the run-up to a boss plays the buildup");

        // the boss wave → the boss track
        app.world_mut().resource_mut::<Wave>().level = 5;
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Boss), "the boss wave plays the boss track");

        // post-boss calm → silence (no music), even though we've advanced past the boss wave
        {
            let mut w = app.world_mut().resource_mut::<Wave>();
            w.level = 6;
            w.calm = BOSS_CALM;
        }
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Silence), "the post-boss calm is silent");
        assert_eq!(app.world_mut().query::<&Music>().iter(app.world()).count(), 0, "nothing is playing during the calm");

        // calm over → back to the main track
        app.world_mut().resource_mut::<Wave>().calm = 0.0;
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main), "the main track resumes after the calm");
    }

    #[test]
    fn right_click_fires_a_chain_beam() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Chain { unlocked: true, charges: 3, recharge: CHAIN_RECHARGE, cooldown: 0.0 });
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Right);
        app.insert_resource(mouse);
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, chain_fire);
        app.update();
        assert_eq!(app.world_mut().query::<&ChainShot>().iter(app.world()).count(), 1, "right-click fires a chain beam");
        assert_eq!(app.world().resource::<Chain>().charges, 2, "and spends a charge");
    }

    #[test]
    fn chain_beam_shatters_rocks_in_its_path() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a beam at the origin spread along Y (segment (0,-58)..(0,58)), a rock sitting on it
        app.world_mut().spawn((ChainShot { life: 1.0, perp: Vec2::new(0.0, 1.0) }, Velocity(Vec2::new(500.0, 0.0)), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 30.0, 0.0)));
        app.add_systems(Update, chain_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 2, "the beam mows the size-2 rock into two size-1 chunks");
    }

    #[test]
    fn shooting_a_mine_shatters_nearby_asteroids() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        // bullet + mine overlapping at the origin (bullet detonates the mine)
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new() },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a size-2 rock out of the bullet's reach but inside the mine's blast,
        // so it can only be broken by the detonation (not the bullet itself).
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(60.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(rocks, 2, "the blast should shatter the size-2 rock into two size-1 chunks, got {rocks}");
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(mines, 0, "the mine detonates and despawns");
    }
}
