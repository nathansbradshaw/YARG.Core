//! YARG-scale scoring and star power. Unity only transports the band's clock.
use crate::DanceNote;
use anyhow::{Result, ensure};
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
pub struct ClockPoint {
    pub time: f64,
    pub beat: f64,
    pub measure: f64,
    pub beats_per_second: f64,
    pub measures_per_second: f64,
}
#[derive(Clone, Copy, Deserialize)]
pub struct PhraseRange {
    pub start: f64,
    pub end: f64,
}
#[derive(Deserialize)]
pub struct Config {
    pub clock: Vec<ClockPoint>,
    pub phrases: Vec<PhraseRange>,
}
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ScoreState {
    pub note: i32,
    pub sustain: i32,
    pub multiplier: i32,
    pub star_power_score: i32,
    pub band_bonus: i32,
    pub score_multiplier: i32,
    pub phrases_hit: i32,
    pub total_phrases: i32,
    pub activations: i32,
    pub active: i32,
    pub base_score: i32,
    pub base_note_score: i32,
    pub total_chords: i32,
    pub phrases_missed: i32,
    pub power: f64,
    pub total_power: f64,
    pub power_time: f64,
    pub power_end: f64,
}
#[derive(Clone)]
struct Phrase {
    remaining: usize,
    failed: bool,
    earned: bool,
    start: f64,
    end: f64,
}
struct Hold {
    index: usize,
    end: f64,
    natural_end: f64,
}
pub struct Ledger {
    clock: Vec<ClockPoint>,
    phrases: Vec<Phrase>,
    note_phrase: Vec<Option<usize>>,
    holds: Vec<Hold>,
    time: f64,
    combo: i32,
    bot: bool,
    allowed: bool,
    pressed: bool,
    band: i32,
    active: bool,
    power: f64,
    gained: f64,
    activations: i32,
    power_time: f64,
    note: f64,
    sustain: f64,
    multiplier: f64,
    power_score: f64,
    band_bonus: f64,
    base_score: i32,
    base_note_score: i32,
    total_chords: i32,
}
impl Ledger {
    pub fn new(notes: &[DanceNote], bot: bool, start: f64, end: f64) -> Self {
        let mut ledger = Self {
            clock: vec![ClockPoint {
                time: 0.,
                beat: 0.,
                measure: 0.,
                beats_per_second: 2.,
                measures_per_second: 0.5,
            }],
            phrases: Vec::new(),
            note_phrase: vec![None; notes.len()],
            holds: Vec::new(),
            time: f64::NEG_INFINITY,
            combo: 0,
            bot,
            allowed: true,
            pressed: false,
            band: 1,
            active: false,
            power: 0.,
            gained: 0.,
            activations: 0,
            power_time: 0.,
            note: 0.,
            sustain: 0.,
            multiplier: 0.,
            power_score: 0.,
            band_bonus: 0.,
            base_score: 0,
            base_note_score: 0,
            total_chords: 0,
        };
        ledger.prepare(notes, &[], start, end);
        ledger
    }
    pub fn configure(
        &mut self,
        config: Config,
        notes: &[DanceNote],
        start: f64,
        end: f64,
    ) -> Result<()> {
        ensure!(!self.time.is_finite(), "Configure scoring before playing");
        ensure!(
            !config.clock.is_empty() && config.clock.len() <= 100_000,
            "Invalid scoring clock"
        );
        ensure!(
            config.clock.iter().all(|p| [
                p.time,
                p.beat,
                p.measure,
                p.beats_per_second,
                p.measures_per_second
            ]
            .iter()
            .all(|v| v.is_finite())
                && p.beats_per_second > 0.
                && p.measures_per_second > 0.),
            "Invalid clock point"
        );
        ensure!(
            config
                .clock
                .windows(2)
                .all(|p| p[0].time < p[1].time && p[0].measure <= p[1].measure),
            "Unsorted scoring clock"
        );
        ensure!(
            config.phrases.len() <= 100_000
                && config
                    .phrases
                    .iter()
                    .all(|p| p.start.is_finite() && p.end.is_finite() && p.end > p.start),
            "Invalid power phrase"
        );
        self.clock = config.clock;
        self.prepare(notes, &config.phrases, start, end);
        Ok(())
    }
    fn point(&self, time: f64) -> &ClockPoint {
        let i = self
            .clock
            .partition_point(|p| p.time <= time)
            .saturating_sub(1);
        &self.clock[i]
    }
    fn beat(&self, time: f64) -> f64 {
        let p = self.point(time);
        p.beat + (time - p.time) * p.beats_per_second
    }
    fn measure(&self, time: f64) -> f64 {
        let p = self.point(time);
        p.measure + (time - p.time) * p.measures_per_second
    }
    fn time_at_measure(&self, measure: f64) -> f64 {
        let i = self
            .clock
            .partition_point(|p| p.measure <= measure)
            .saturating_sub(1);
        let p = &self.clock[i];
        p.time + (measure - p.measure) / p.measures_per_second
    }
    fn prepare(&mut self, notes: &[DanceNote], ranges: &[PhraseRange], start: f64, end: f64) {
        self.phrases.clear();
        self.note_phrase.fill(None);
        let eligible: Vec<_> = notes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.judgable
                    && n.kind != yace_stepmania::NoteKind::Mine
                    && n.time >= start
                    && n.time < end
            })
            .collect();
        let mut groups = std::collections::HashSet::new();
        let (mut base, mut plain) = (0_f64, 0_f64);
        for (_, n) in &eligible {
            groups.insert((n.beat * 48.).round() as i64);
            // YARG's star baseline uses the combo before each chart row.
            let multiplier = ((groups.len() - 1) / 10 + 1).min(4) as f64;
            let sustain = if n.end_time > n.time {
                (self.beat(n.end_time.min(end)) - self.beat(n.time)).max(0.) * 25.
            } else {
                0.
            };
            plain += 50. + sustain;
            base += (50. + sustain) * multiplier;
        }
        self.total_chords = groups.len() as i32;
        self.base_score = base.round() as i32;
        self.base_note_score = plain.round() as i32;
        let mut generated = Vec::new();
        if ranges.is_empty() {
            if let (Some((_, first)), Some((_, last))) = (eligible.first(), eligible.last()) {
                let mut beat = (first.beat / 4.).floor() * 4.;
                while beat <= last.beat {
                    // Eight-beat phrases every 32 source beats.
                    let indices: Vec<_> = eligible
                        .iter()
                        .filter(|(_, n)| n.beat >= beat && n.beat < beat + 8.)
                        .map(|(i, _)| *i)
                        .collect();
                    if !indices.is_empty() {
                        generated.push(indices);
                    }
                    beat += 32.;
                }
            }
        } else {
            for range in ranges {
                let indices: Vec<_> = eligible
                    .iter()
                    .filter(|(_, n)| n.time >= range.start && n.time < range.end)
                    .map(|(i, _)| *i)
                    .collect();
                if !indices.is_empty() {
                    generated.push(indices);
                }
            }
        }
        for indices in generated {
            let id = self.phrases.len();
            let indices: Vec<_> = indices
                .into_iter()
                .filter(|i| self.note_phrase[*i].is_none())
                .collect();
            if indices.is_empty() {
                continue;
            }
            for &i in &indices {
                self.note_phrase[i] = Some(id);
            }
            self.phrases.push(Phrase {
                remaining: indices.len(),
                failed: false,
                earned: false,
                start: notes[*indices.first().unwrap()].time,
                end: notes[*indices.last().unwrap()].time,
            });
        }
    }
    fn combo_multiplier(&self) -> i32 {
        (self.combo / 10 + 1).min(4)
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn state(&self) -> ScoreState {
        // Integration can land a few ulps below an exact half point when the
        // same hold is divided across different render/input frame boundaries.
        let points = |value: f64| (value + 1e-7).round() as i32;
        ScoreState {
            note: points(self.note),
            sustain: points(self.sustain),
            multiplier: points(self.multiplier),
            star_power_score: points(self.power_score),
            band_bonus: points(self.band_bonus),
            score_multiplier: self.combo_multiplier() * if self.active { 2 } else { 1 },
            phrases_hit: self.phrases.iter().filter(|p| p.earned).count() as i32,
            total_phrases: self.phrases.len() as i32,
            phrases_missed: self.phrases.iter().filter(|p| p.failed).count() as i32,
            activations: self.activations,
            active: i32::from(self.active),
            base_score: self.base_score,
            base_note_score: self.base_note_score,
            total_chords: self.total_chords,
            power: self.power,
            total_power: self.gained,
            power_time: self.power_time,
            power_end: if self.active {
                self.time_at_measure(self.measure(self.time) + self.power * 8.)
            } else {
                f64::INFINITY
            },
        }
    }
    pub fn score(&self) -> i32 {
        let s = self.state();
        s.note
            .saturating_add(s.sustain)
            .saturating_add(s.multiplier)
            .saturating_add(s.star_power_score)
    }
    fn add(&mut self, points: f64, sustain: bool) {
        let combo = self.combo_multiplier() as f64;
        if sustain {
            self.sustain += points;
        } else {
            self.note += points;
        }
        self.multiplier += points * (combo - 1.);
        if self.active {
            self.power_score += points * combo;
        }
        let bonus = (self.band - if self.active { 2 } else { 1 }).max(0) as f64;
        self.band_bonus += points * combo * bonus;
    }
    pub fn advance(&mut self, time: f64) {
        if !self.time.is_finite() {
            self.time = time;
            return;
        }
        if time <= self.time {
            return;
        }
        while self.time < time {
            let mut next = time;
            if self.active {
                next = next.min(self.time_at_measure(self.measure(self.time) + self.power * 8.));
            }
            if let Some(p) = self.clock.iter().find(|p| p.time > self.time + 1e-9) {
                next = next.min(p.time);
            }
            for hold in &self.holds {
                if hold.end > self.time {
                    next = next.min(hold.end);
                }
            }
            if next <= self.time + 1e-9 {
                self.time = next.max(self.time);
                self.holds.retain(|h| h.natural_end > self.time);
                self.active = false;
                self.power = 0.;
                continue;
            }
            let beats = (self.beat(next) - self.beat(self.time)).max(0.);
            let count = self.holds.iter().filter(|h| h.end > self.time).count();
            self.add(beats * 25. * count as f64, true);
            if self.active {
                self.power = (self.power
                    - (self.measure(next) - self.measure(self.time)).max(0.) / 8.)
                    .max(0.);
                self.power_time += next - self.time;
                if self.power < 1e-8 {
                    self.power = 0.;
                    self.active = false;
                }
            }
            self.time = next;
            self.holds.retain(|h| h.natural_end > next);
        }
    }
    pub fn hit(&mut self, index: usize, note: &DanceNote, time: f64, combo: i32, end: f64) {
        self.advance(time);
        self.combo = combo;
        self.add(50., false);
        if note.end_time > time {
            self.holds.push(Hold {
                index,
                end: note.end_time.min(end),
                natural_end: note.end_time.min(end),
            });
        }
        if let Some(id) = self.note_phrase[index] {
            let p = &mut self.phrases[id];
            p.remaining = p.remaining.saturating_sub(1);
            if self.allowed && p.remaining == 0 && !p.failed && !p.earned {
                p.earned = true;
                self.power = (self.power + 0.25).min(1.);
                self.gained += 0.25;
            }
        }
        self.try_activate();
    }
    pub fn fail(&mut self, index: usize, time: f64) {
        self.advance(time);
        self.combo = 0;
        self.holds.retain(|h| h.index != index);
        if let Some(id) = self.note_phrase[index]
            && !self.phrases[id].earned
        {
            self.phrases[id].failed = true;
        } else if self.note_phrase[index].is_none() {
            // Mines are excluded from required heads, but triggering one during
            // an unfinished phrase breaks it like an overhit.
            for phrase in &mut self.phrases {
                if !phrase.earned && time >= phrase.start && time <= phrase.end {
                    phrase.failed = true;
                }
            }
        }
    }
    pub fn limit_hold(&mut self, index: usize, end: f64) {
        for hold in &mut self.holds {
            if hold.index == index {
                hold.end = hold.natural_end.min(end);
            }
        }
    }
    pub fn context(&mut self, band: i32, allowed: bool) {
        self.band = band.max(1);
        self.allowed = allowed;
    }
    pub fn power_input(&mut self, time: f64, pressed: bool) {
        self.advance(time);
        self.pressed = pressed;
        self.try_activate();
    }
    fn try_activate(&mut self) {
        if self.allowed && !self.active && self.power >= 0.5 && (self.bot || self.pressed) {
            self.active = true;
            self.activations += 1;
        }
    }
    pub fn is_phrase(&self, index: usize) -> bool {
        self.note_phrase
            .get(index)
            .and_then(|p| *p)
            .is_some_and(|id| !self.phrases[id].failed)
    }
}
