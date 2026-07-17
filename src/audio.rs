//! Procedural techno-club soundtrack.
//!
//! We synthesize a seamless 8-bar loop straight into a 16-bit mono WAV byte buffer
//! at startup — no asset file, nothing to download — and hand it to Bevy's audio as
//! a looping `AudioSource`. Everything here is plain `std` math (no Bevy types) so
//! it stays testable and decoupled from the engine.
//!
//! The "club" vibe = a fast four-on-the-floor kick, an offbeat sub bass in A-minor,
//! closed + open hats, a backbeat clap, and a relentless 16th-note detuned-saw arp
//! that drives the whole loop. Voice tails wrap the buffer end so the loop is seamless.

use std::f32::consts::TAU;

const SR: f32 = 44_100.0; // sample rate

// --- deterministic value noise (seeded by sample index so the loop matches itself) ---
fn noise(i: usize) -> f32 {
    let mut x = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(1_013_904_223);
    x ^= x >> 13;
    x = x.wrapping_mul(1_274_126_177);
    x ^= x >> 16;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

// --- one-shot voices: `t` is seconds since the hit was triggered ---

// Punchy kick: a sine whose pitch drops 130 → 48 Hz, with a fast amplitude decay
// and a touch of saturation so it thumps.
fn kick(t: f32) -> f32 {
    let env = (-t * 9.0).exp();
    let phase = TAU * (48.0 * t + (82.0 / 30.0) * (1.0 - (-30.0 * t).exp()));
    (phase.sin() * env * 1.3).tanh()
}

// Hi-hat: high-passed noise (difference of two noise taps) with a short decay.
// `open` hats ring longer than the ticking closed hats.
fn hat(t: f32, i: usize, open: bool) -> f32 {
    let decay = if open { 22.0 } else { 95.0 };
    (noise(i) - noise(i + 7)) * 0.5 * (-t * decay).exp()
}

// Clap: three quick noise bursts in a row give the hand-clap texture.
fn clap(t: f32, i: usize) -> f32 {
    let e = (-t * 42.0).exp()
        + 0.6 * (-(t - 0.008).max(0.0) * 55.0).exp()
        + 0.4 * (-(t - 0.016).max(0.0) * 60.0).exp();
    noise(i) * e * 0.4
}

// Sub bass: a sine plus a quiet octave, quick attack, medium decay.
fn bass(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 60.0).exp()) * (-t * 3.0).exp();
    ((TAU * f * t).sin() + 0.25 * (TAU * 2.0 * f * t).sin()) * env
}

// --- oscillator shapes (phase `ph` in cycles) ---
fn saw(ph: f32) -> f32 {
    2.0 * (ph - (ph + 0.5).floor())
}
fn square(ph: f32) -> f32 {
    if ph - ph.floor() < 0.5 {
        1.0
    } else {
        -1.0
    }
}
// --- melodic voices (plain fns now that there's one main track + the boss track) ---
// Supersaw arp stab — the main track's driving 16th-note melody.
fn arp_saw(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 120.0).exp()) * (-t * 7.0).exp();
    (saw(f * t) + saw(f * 1.006 * t)) * 0.25 * env
}
// Hollow square arp — the boss track's harsher lead.
fn arp_square(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 120.0).exp()) * (-t * 8.0).exp();
    square(f * t) * 0.3 * env
}
// Rave chord STAB — a bright, plucky detuned-supersaw hit (played as a chord). The club/rave
// signature that drives the drops, in place of a sung lead melody.
fn stab_voice(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 200.0).exp()) * (-t * 9.0).exp();
    (saw(f * t) + saw(f * 1.008 * t) + saw(f * 0.992 * t)) * 0.13 * env
}
// Growly reese bass — detuned saws over the fundamental (the boss's dirty low end).
fn reese(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 50.0).exp()) * (-t * 3.0).exp();
    ((TAU * f * t).sin() * 0.6 + (saw(f * t) + saw(f * 1.008 * t)) * 0.22) * env
}
// A bright crash-cymbal hit (long noise decay) to punctuate a drop landing.
fn crash(t: f32, i: usize) -> f32 {
    (noise(i) - noise(i + 11)) * 0.5 * (-t * 3.2).exp()
}

// Sustained detuned-saw PAD chord — a slow-swelling atmospheric bed. Tracks that use this feel
// harmonic and spacious instead of arp-driven, which is a bigger identity change than a waveform
// swap. Held across a whole 2-bar phrase.
fn pad_voice(t: f32, f: f32) -> f32 {
    let env = (1.0 - (-t * 5.0).exp()) * (-t * 0.7).exp(); // slow attack, long tail
    (saw(f * t) + saw(f * 1.005 * t) + saw(f * 0.995 * t)) * 0.09 * env
}

// Add a voice into the buffer starting at `start`, wrapping past the end so the
// hit's tail bleeds into the loop start (seamless looping).
fn add_voice(buf: &mut [f32], start: usize, dur: f32, gain: f32, mut voice: impl FnMut(f32, usize) -> f32) {
    let len = buf.len();
    let count = (dur * SR) as usize;
    let fade = (0.004 * SR) as usize; // 4 ms release so a truncated tail never clicks
    for i in 0..count {
        let idx = (start + i) % len;
        let rel = if i + fade > count { (count - i) as f32 / fade as f32 } else { 1.0 };
        buf[idx] += voice(i as f32 / SR, i) * gain * rel;
    }
}

// Normalize → gentle saturation → 16-bit PCM WAV. Shared master stage for both tracks.
fn master(buf: &[f32]) -> Vec<u8> {
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| ((v * norm).tanh() * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// The main soundtrack: a full-length (~2 min) A-minor track with a real arrangement
/// (intro → drop → breakdown → build → 2nd drop → outro) that LOOPS seamlessly. Because it loops
/// it is never faded — so it doesn't cut off. This is the single normal-play track.
pub fn main_track_wav() -> Vec<u8> {
    let bpm = 128.0;
    let bars = 64usize;
    let step = 60.0 / bpm / 4.0;
    let steps = bars * 16;
    let ssamp = step * SR;
    let n = (steps as f32 * ssamp).round() as usize;
    // TWO buses so we can sidechain: `drums` isn't ducked; `music` is pumped by the kick.
    let mut drums = vec![0f32; n];
    let mut music = vec![0f32; n];

    // Am F Dm E (i–VI–iv–V) — a dramatic MINOR progression (not the old bright, bouncy F–C–G).
    let prog = [55.00f32, 43.65, 73.42, 82.41];
    // arp kept within one octave (no bright octave leap) so it drives hypnotically; ♭6 (8) darkens.
    let arp = [0i32, 3, 7, 8, 7, 5, 3, 0];

    for s in 0..steps {
        let start = (s as f32 * ssamp) as usize;
        let bar = s / 16;
        let bp = s % 16;
        let beat = s % 4;
        let root = prog[(bar / 2) % 4];

        // arrangement sections
        let intro = bar < 8;
        let drop_a = (8..24).contains(&bar);
        let brk = (24..32).contains(&bar);
        let build = (32..40).contains(&bar);
        let drop_b = (40..56).contains(&bar);
        let outro = bar >= 56;
        let drop = drop_a || drop_b;
        let energetic = drop || build;
        let has_drums = energetic || (intro && bar >= 6) || (outro && bar < 60); // build in, fade out

        // ── DRUM BUS (not pumped) ──
        if has_drums && beat == 0 {
            add_voice(&mut drums, start, 0.34, 0.95, |t, _| kick(t)); // four-on-floor
        }
        if energetic && (bp == 4 || bp == 12) {
            add_voice(&mut drums, start, 0.20, 0.5, clap); // backbeat
        }
        if energetic {
            let open = beat == 2;
            let (dur, gain) = if open { (0.14, 0.18) } else { (0.05, 0.12) };
            add_voice(&mut drums, start, dur, gain, move |t, i| hat(t, i, open)); // 16th hats
        } else if brk && beat == 2 {
            add_voice(&mut drums, start, 0.14, 0.14, move |t, i| hat(t, i, true));
        }
        if (bar == 8 || bar == 40) && bp == 0 {
            add_voice(&mut drums, start, 0.9, 0.4, crash); // drop hit
        }
        if bar == 32 && bp == 0 {
            let riser_dur = step * 128.0; // 8-bar swell into the 2nd drop
            add_voice(&mut drums, start, riser_dur, 0.2, move |t, i| {
                let p = (t / riser_dur).min(1.0);
                noise(i * 2) * p * p
            });
        }
        if build && bar >= 38 {
            let rp = (s - 38 * 16) as f32 / 32.0; // snare roll ramps over the last 2 build bars
            add_voice(&mut drums, start, 0.08, 0.1 + 0.3 * rp, move |t, i| (noise(i) - noise(i + 3)) * 0.5 * (-t * 30.0).exp());
        }

        // ── MUSIC BUS (pumped) ──
        if (energetic || (intro && bar >= 4) || (outro && bar < 60)) && beat == 2 {
            add_voice(&mut music, start, step * 2.0, 0.6, move |t, _| bass(t, root)); // offbeat sub
        }
        if (intro || brk || outro) && s.is_multiple_of(32) {
            let base = root * 2.0;
            for semi in [0.0f32, 3.0, 7.0] {
                let f = base * 2f32.powf(semi / 12.0);
                add_voice(&mut music, start, step * 32.0, 0.5, move |t, _| pad_voice(t, f)); // breakdown pad
            }
        }
        // ARP supersaw — 16ths when energetic, softer 8ths elsewhere (the hypnotic drive)
        let play_arp = if energetic { true } else { s.is_multiple_of(2) };
        if play_arp {
            let idx = if energetic { s } else { s / 2 };
            let f = root * 4.0 * 2f32.powf(arp[idx % 8] as f32 / 12.0);
            let g = if energetic { 0.15 } else { 0.10 };
            add_voice(&mut music, start, step * 1.6, g, move |t, _| arp_saw(t, f));
        }
        // RAVE STAB — supersaw chord hits on a syncopated pattern during the drops (the club hook)
        if drop && (bp == 0 || bp == 6 || bp == 10) {
            let base = root * 2.0;
            for semi in [0.0f32, 3.0, 7.0] {
                let f = base * 2f32.powf(semi / 12.0);
                add_voice(&mut music, start, step * 3.0, 0.16, move |t, _| stab_voice(t, f));
            }
        }
    }

    // Mix with a SIDECHAIN PUMP: during the four-on-floor sections the music bus ducks on each
    // beat and swells back — the "breathing" that defines the club/EDM sound.
    let beat_dur = step * 4.0;
    let mut out = vec![0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let bar = (t / (step * 16.0)) as usize;
        let energetic = (8..24).contains(&bar) || (32..56).contains(&bar);
        let pump = if energetic {
            let ph = (t % beat_dur) / beat_dur; // 0 = on the kick
            0.35 + 0.65 * (ph / 0.85).min(1.0)
        } else {
            1.0
        };
        *o = drums[i] + music[i] * pump;
    }

    master(&out)
}

/// The boss track — its own beast: a relentless POUND kick (every 8th), a tritone-laced menacing
/// arp over a tense A→E♭→F→E progression, a dark pad drone and a square lead an octave up. A
/// tight 16-bar loop, seamless (no fade).
pub fn boss_track_wav() -> Vec<u8> {
    let bpm = 152.0;
    let bars = 16usize;
    let step = 60.0 / bpm / 4.0;
    let steps = bars * 16;
    let ssamp = step * SR;
    let n = (steps as f32 * ssamp).round() as usize;
    let mut buf = vec![0f32; n];

    let prog = [55.00f32, 77.78, 43.65, 82.41]; // A → E♭ (tritone) → F → E
    let arp = [0i32, 6, 7, 6, 12, 8, 6, 3]; // ♯4/♭5 (6) + ♭6 (8) = menace

    for s in 0..steps {
        let start = (s as f32 * ssamp) as usize;
        let bp = s % 16;
        let root = prog[(s / 32) % 4]; // 2-bar phrases

        if s.is_multiple_of(2) {
            add_voice(&mut buf, start, 0.30, 0.95, |t, _| kick(t)); // POUND — every 8th
            add_voice(&mut buf, start, step * 2.0, 0.55, move |t, _| reese(t, root)); // reese every 8th
        }
        if bp == 4 || bp == 12 {
            add_voice(&mut buf, start, 0.20, 0.5, clap);
        }
        {
            let open = (s % 4) == 2;
            let (dur, gain) = if open { (0.12, 0.16) } else { (0.05, 0.11) };
            add_voice(&mut buf, start, dur, gain, move |t, i| hat(t, i, open));
        }
        if s.is_multiple_of(32) {
            let base = root * 2.0;
            for semi in [0.0f32, 3.0, 7.0] {
                let f = base * 2f32.powf(semi / 12.0);
                add_voice(&mut buf, start, step * 32.0, 0.45, move |t, _| pad_voice(t, f)); // dark drone
            }
        }
        // square arp every 16th
        {
            let f = root * 4.0 * 2f32.powf(arp[s % 8] as f32 / 12.0);
            add_voice(&mut buf, start, step * 1.6, 0.16, move |t, _| arp_square(t, f));
        }
        // screaming lead an octave up on the phrase downbeat + a syncopation
        if bp == 0 || bp == 11 {
            let f = root * 8.0 * 2f32.powf(arp[(s / 2) % 8] as f32 / 12.0);
            add_voice(&mut buf, start, step * 3.0, 0.14, move |t, _| arp_square(t, f));
        }
    }

    master(&buf)
}

/// A ~10 s tension RISER played in the run-up to a boss wave, so the boss doesn't slam in cold:
/// a low A drone that swells, a noise sweep whose cutoff climbs, and heartbeat kicks that speed
/// up and get louder. Crescendos into the boss loop (one-shot, not faded).
pub fn boss_buildup_wav() -> Vec<u8> {
    let dur = 10.0;
    let n = (dur * SR) as usize;
    let mut buf = vec![0f32; n];

    // per-sample bed: a swelling low drone + a rising-cutoff noise sweep
    let mut lp = 0.0f32;
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let p = t / dur; // 0..1
        let drone = ((TAU * 55.0 * t).sin() * 0.5 + saw(55.0 * t) * 0.15) * (0.15 + 0.55 * p);
        let fc = 200.0 * (5000.0f32 / 200.0).powf(p); // cutoff climbs 200 → 5000 Hz
        lp += (1.0 - (-TAU * fc / SR).exp()) * (noise(i) - lp);
        let sweep = lp * (0.08 + 0.5 * p * p); // swells toward the drop
        *out = drone + sweep;
    }

    // heartbeat kicks that accelerate and intensify as the boss nears
    let mut kt = 0.0f32;
    let mut interval = 0.6f32;
    while kt < dur - 0.25 {
        let start = (kt * SR) as usize;
        let g = 0.5 + 0.5 * (kt / dur);
        add_voice(&mut buf, start, 0.30, g, |t, _| kick(t));
        interval = (interval * 0.9).max(0.14);
        kt += interval;
    }

    master(&buf)
}

// Minimal 16-bit mono PCM WAV container.
fn wav_bytes(samples: &[i16], sr: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // channels = mono
    v.extend_from_slice(&sr.to_le_bytes());
    v.extend_from_slice(&(sr * 2).to_le_bytes()); // byte rate = sr * channels * 2
    v.extend_from_slice(&2u16.to_le_bytes()); // block align = channels * 2
    v.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

// ─────────────────────────────── one-shot sound effects ───────────────
// Short procedural WAVs (no asset files). Each renders `dur` seconds via a per-sample
// closure, soft-clips, and packs to a mono WAV — played once via Bevy's AudioPlayer.
fn render_sfx(dur: f32, mut voice: impl FnMut(f32, usize) -> f32) -> Vec<u8> {
    let n = (dur * SR) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            ((voice(t, i)).clamp(-1.0, 1.0) * i16::MAX as f32 * 0.9) as i16
        })
        .collect();
    wav_bytes(&samples, SR as u32)
}

/// Ship firing: a short descending "pew" — a saw whose pitch drops fast.
pub fn fire_sfx_wav() -> Vec<u8> {
    render_sfx(0.14, |t, _| {
        let freq = 380.0 + 900.0 * (-t * 45.0).exp();
        saw(freq * t) * (-t * 26.0).exp() * 0.5
    })
}

/// Asteroid breaking — a faithful port of the JS `playBreak`: a white-noise burst through a
/// LOWPASS whose cutoff sweeps DOWNWARD, giving a deep filtered "boom/whoosh" (no tone, so no
/// woodblock "tok"). Size-aware like the JS: a big rock uses a low cutoff (deep boom), a small
/// one a high cutoff (crack). `size` is 1 (small) … 3 (large).
///
/// The cutoff has to move sample-to-sample, which needs filter state, so this can't use the
/// stateless `render_sfx` — we run two cascaded one-pole lowpasses (≈ the JS biquad's rolloff)
/// by hand, then normalize (a low cutoff passes little energy, so levels vary by size).
pub fn break_sfx_wav(size: u8) -> Vec<u8> {
    let sz = size.clamp(1, 3) as f32;
    let f0 = 520.0 + (3.0 - sz) * 430.0; // size3 ~520 Hz (boom) … size1 ~1380 Hz (crack)
    let f1 = (f0 * 0.3).max(120.0); // cutoff glides down to here over `sweep`
    let dur = 0.12 + sz * 0.05; // size3 ~0.27 s … size1 ~0.17 s
    let sweep = 0.18;
    let n = (dur * SR) as usize;
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    let mut buf = vec![0f32; n];
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // exponential cutoff glide f0 → f1 over `sweep` seconds
        let fc = f0 * (f1 / f0).powf((t / sweep).min(1.0));
        let alpha = 1.0 - (-TAU * fc / SR).exp();
        let x = noise(i); // white noise
        lp1 += alpha * (x - lp1);
        lp2 += alpha * (lp1 - lp2);
        // gain env: ~6 ms attack, exp decay to ≈0 at `dur` (matches the JS ramps)
        let env = (t / 0.006).min(1.0) * (-t * (9.21 / dur)).exp();
        *out = lp2 * env;
    }
    // normalize to a consistent peak — the 2-pole lowpass output level drops with the cutoff
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| (v * norm * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// Mine explosion: a big, punchy detonation. A sharp CRACK snap on the attack, a deep sine
/// BOOM sweeping down in pitch, and a broadband NOISE blast — driven HARD into saturation so
/// it lands heavy instead of a soft, clean sine.
pub fn mine_sfx_wav() -> Vec<u8> {
    render_sfx(0.55, |t, i| {
        // CRACK: sharp bright noise transient on the detonation attack
        let crack = (noise(i) - noise(i + 2)) * (-t * 45.0).exp();
        // BOOM: deep sine sweeping 160 → 28 Hz — deeper, subbier floor than the old 220→35
        let freq = 28.0 + 132.0 * (-t * 11.0).exp();
        let boom = (TAU * freq * t).sin() * (-t * 5.0).exp();
        // BLAST: broadband noise roar that fills out the explosion
        let blast = (noise(i) - noise(i + 5)) * 0.5 * (-t * 8.0).exp();
        // overdrive the sum into saturation for a loud, heavy landing; boom weighted up for depth
        ((boom * 1.5 + crack * 0.7 + blast * 0.55) * 1.8).tanh()
    })
}

/// Ship destroyed: a dramatic descending "doom" — a tone falling 420 → 60 Hz (the death cry)
/// over an explosion burst and a deep sub thump. Longer and more mournful than a mine blast so
/// losing a life reads as a bigger deal. The falling pitch uses an integral-phase sweep (like
/// the kick) so it glides cleanly instead of warbling.
pub fn death_sfx_wav() -> Vec<u8> {
    render_sfx(0.6, |t, i| {
        // descending doom: instantaneous freq 420 → 60 Hz; phase = ∫f dt in cycles
        let cyc = 60.0 * t + 80.0 * (1.0 - (-4.5 * t).exp());
        let doom = ((TAU * cyc).sin() * 0.6 + saw(cyc) * 0.3) * (-t * 4.0).exp();
        // explosion burst on the attack (broadband noise)
        let blast = (noise(i) - noise(i + 4)) * 0.5 * (-t * 7.0).exp();
        // deep sub thump underneath for weight
        let sub = (TAU * 45.0 * t).sin() * (-t * 6.0).exp();
        ((doom + blast * 0.7 + sub * 0.8) * 1.3).tanh()
    })
}

/// Enemy mob firing: a low, buzzy descending blip — hostile and clearly NOT the player's
/// brighter square-wave "pew" (which sweeps 1280→380 Hz). This one sits down at 460→120 Hz.
pub fn enemy_shot_wav() -> Vec<u8> {
    render_sfx(0.13, |t, _| {
        let f = 120.0 + 340.0 * (-t * 30.0).exp(); // 460 → 120 Hz
        saw(f * t) * (-t * 22.0).exp() * 0.45
    })
}

/// Enemy mob destroyed: a small zap-pop — a quick descending tone plus a noise burst. Lighter
/// and shorter than the player-ship death (which is a long mournful doom), so a mob popping
/// reads as a minor event.
pub fn enemy_die_wav() -> Vec<u8> {
    render_sfx(0.28, |t, i| {
        let f = 90.0 + 300.0 * (-t * 18.0).exp(); // 390 → 90 Hz
        let tone = (TAU * f * t).sin() * (-t * 12.0).exp();
        let burst = (noise(i) - noise(i + 3)) * 0.5 * (-t * 16.0).exp();
        ((tone * 0.7 + burst * 0.6) * 1.4).tanh()
    })
}

/// Warp launch — a port of the JS `playVortex`: two tones PLUNGING in pitch (saw 640→52 Hz,
/// sine 1020→80 Hz) under a swept band-passed NOISE whoosh (center 1900→220 Hz), so it reads as
/// a portal tearing open. Distinct from every other effect. Swept filters need state, so this
/// runs its own sample loop rather than the stateless `render_sfx`.
pub fn warp_wav() -> Vec<u8> {
    let dur = 0.56;
    let n = (dur * SR) as usize;
    let mut buf = vec![0f32; n];
    let (mut la, mut lb) = (0.0f32, 0.0f32); // two one-poles → a crude swept bandpass (hi − lo)
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // descending tones: instantaneous f = f_end + (f0−f_end)·e^(−6t); phase (cycles) = ∫f dt
        let cyc1 = 52.0 * t + (640.0 - 52.0) / 6.0 * (1.0 - (-6.0 * t).exp());
        let cyc2 = 80.0 * t + (1020.0 - 80.0) / 6.0 * (1.0 - (-6.0 * t).exp());
        let tone = saw(cyc1) * 0.5 + (TAU * cyc2).sin() * 0.5;
        // swept-noise whoosh: band center glides 1900 → 220 Hz
        let fc = 1900.0 * (220.0f32 / 1900.0).powf((t / 0.5).min(1.0));
        let x = noise(i);
        la += (1.0 - (-TAU * (fc * 1.6) / SR).exp()) * (x - la);
        lb += (1.0 - (-TAU * (fc * 0.6) / SR).exp()) * (x - lb);
        let whoosh = (la - lb) * 2.0;
        let tone_env = (t / 0.03).min(1.0) * (-t * 5.0).exp();
        let whoosh_env = (t / 0.05).min(1.0) * (-t * 5.5).exp();
        *out = tone * tone_env * 0.6 + whoosh * whoosh_env * 0.5;
    }
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| (v * norm * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// Achievement unlocked: a bright rolled major arpeggio (C6-E6-G6-C7, each note entering slightly
/// later so they ring together) — a positive, sparkly "flourish".
pub fn achievement_sfx_wav() -> Vec<u8> {
    render_sfx(0.7, |t, _| {
        let voice = |f: f32, delay: f32| {
            if t < delay {
                return 0.0;
            }
            let nt = t - delay;
            (TAU * f * nt).sin() * (1.0 - (-nt * 80.0).exp()) * (-nt * 3.5).exp()
        };
        (voice(1046.5, 0.0) + voice(1318.5, 0.06) + voice(1568.0, 0.12) + voice(2093.0, 0.18)) * 0.25
    })
}

// A bright, fast six-note ascending run — a classic "1UP" jingle for the gold rock. Deliberately
// quicker, higher and sparklier than the achievement chime so an extra life reads as its own event.
pub fn life_sfx_wav() -> Vec<u8> {
    let notes = [783.99, 1046.5, 1318.5, 1568.0, 2093.0, 2637.0]; // G5 C6 E6 G6 C7 E7
    render_sfx(0.6, |t, _| {
        let mut s = 0.0;
        for (i, &f) in notes.iter().enumerate() {
            let delay = i as f32 * 0.07;
            if t >= delay {
                let nt = t - delay;
                let env = (1.0 - (-nt * 120.0).exp()) * (-nt * 9.0).exp(); // fast pluck, quick decay
                s += ((TAU * f * nt).sin() + 0.3 * (TAU * 2.0 * f * nt).sin()) * env; // + a shimmering octave
            }
        }
        s * 0.22
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_are_wav_and_nonsilent() {
        // the main track + the boss track must both be valid, audible loops
        for wav in [main_track_wav(), boss_track_wav()] {
            assert_eq!(&wav[0..4], b"RIFF", "starts with a RIFF header");
            assert_eq!(&wav[8..12], b"WAVE", "is a WAVE file");
            // enough audio for a full loop (main ~2 min, boss ~25 s at 44.1 kHz mono 16-bit)
            assert!(wav.len() > 44 + 600_000, "buffer should hold the whole loop, got {}", wav.len());
            // the PCM must not be all zeros (something actually got synthesized)
            let loud = wav[44..].chunks_exact(2).any(|b| i16::from_le_bytes([b[0], b[1]]).abs() > 3000);
            assert!(loud, "the track should contain audible samples");
        }
    }

    #[test]
    fn sfx_are_valid_nonsilent_wavs() {
        for wav in [
            fire_sfx_wav(),
            break_sfx_wav(1),
            break_sfx_wav(2),
            break_sfx_wav(3),
            mine_sfx_wav(),
            death_sfx_wav(),
            enemy_shot_wav(),
            enemy_die_wav(),
            warp_wav(),
            achievement_sfx_wav(),
            life_sfx_wav(),
        ] {
            assert_eq!(&wav[0..4], b"RIFF", "sfx starts with a RIFF header");
            assert_eq!(&wav[8..12], b"WAVE", "sfx is a WAVE file");
            assert!(wav.len() > 44 + 2000, "sfx should carry audio data, got {}", wav.len());
            let loud = wav[44..].chunks_exact(2).any(|b| i16::from_le_bytes([b[0], b[1]]).abs() > 2000);
            assert!(loud, "sfx should contain audible samples");
        }
    }
}
