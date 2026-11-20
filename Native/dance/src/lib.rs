use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
};
use yace_stepmania::{
    Document, Format, NoteKind, Timing, TimingKind,
    timing::{AudioPosition, Boundary},
};
pub mod scoring;
pub mod skin;

pub const WINDOW: f64 = 0.180;
const HOLD_GRACE: f64 = 0.250;
const ROLL_GAP: f64 = 0.500;
const MINE_WINDOW: f64 = 0.090;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DanceNote {
    pub lane: u8,
    pub kind: NoteKind,
    pub beat: f64,
    pub end_beat: f64,
    pub time: f64,
    pub end_time: f64,
    pub judgable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DanceChart {
    pub name: String,
    pub difficulty: u8,
    pub meter: u32,
    pub timing: Timing,
    pub notes: Vec<DanceNote>,
}

#[derive(Debug, Serialize)]
pub struct DanceSong {
    pub title: String,
    pub artist: String,
    pub music: String,
    pub banner: String,
    pub charts: Vec<DanceChart>,
}

pub fn parse(text: String, ssc: bool) -> Result<DanceSong> {
    parse_for_clock(text, ssc, 0.)
}
pub fn parse_for_clock(text: String, ssc: bool, clock_offset: f64) -> Result<DanceSong> {
    ensure!(clock_offset.is_finite(), "Invalid band audio offset");
    let mut document = Document::parse(text, if ssc { Format::Ssc } else { Format::Sm })?;
    // YARG song time = recording time - song.ini delay. Convert all dance
    // clocks together; never bake stops or warps into the band's tempo map.
    document.timing.offset += clock_offset;
    for chart in &mut document.charts {
        if let Some(timing) = &mut chart.timing {
            timing.offset += clock_offset;
        }
    }
    let mut charts = Vec::new();
    for chart in &document.charts {
        if chart.stepstype != "dance-single" || chart.read_only_reason.is_some() {
            continue;
        }
        let difficulty = match chart.difficulty.as_str() {
            "Beginner" => 0,
            "Easy" => 1,
            "Medium" => 2,
            "Hard" => 3,
            "Challenge" => 4,
            "Edit" => 5,
            _ => continue,
        };
        ensure!(
            !charts
                .iter()
                .any(|c: &DanceChart| c.difficulty == difficulty),
            "Multiple {} charts: select one in YACE and export it separately",
            chart.difficulty
        );
        let timing = document.effective_timing(&chart.id)?.clone();
        let mut notes: Vec<_> = chart
            .notes
            .iter()
            .map(|n| {
                let beat = n.row as f64 / 48.;
                let time = timing.seconds(beat, Boundary::Note);
                DanceNote {
                    lane: n.lane,
                    kind: n.kind,
                    beat,
                    time,
                    end_beat: n.end_row.map_or(beat, |r| r as f64 / 48.),
                    end_time: n
                        .end_row
                        .map_or(time, |r| timing.seconds(r as f64 / 48., Boundary::Note)),
                    judgable: timing.judgable(beat) && n.kind != NoteKind::Fake,
                }
            })
            .collect();
        // Warps can put distinct rows at the same audio time; stable lane ordering
        // makes replay and bot processing deterministic.
        notes.sort_by(|a, b| {
            a.time
                .total_cmp(&b.time)
                .then(a.lane.cmp(&b.lane))
                .then(a.beat.total_cmp(&b.beat))
        });
        charts.push(DanceChart {
            name: chart.description.clone(),
            difficulty,
            meter: chart.meter,
            timing,
            notes,
        });
    }
    ensure!(
        !charts.is_empty(),
        "No playable dance-single charts in this simfile"
    );
    let meta = |key: &str| document.metadata.get(key).cloned().unwrap_or_default();
    Ok(DanceSong {
        title: meta("TITLE"),
        artist: meta("ARTIST"),
        music: meta("MUSIC"),
        banner: meta("BANNER"),
        charts,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub score: i32,
    pub hits: i32,
    pub combo: i32,
    pub max_combo: i32,
    pub misses: i32,
    pub drops: i32,
    pub mines: i32,
    pub holds: i32,
    pub total: i32,
    pub judgment: i32,
    pub sequence: i32,
    pub next_note: i32,
    pub offset: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderNote {
    pub head: f32,
    pub tail: f32,
    pub lane: i32,
    pub kind: i32,
    pub state: i32,
    pub index: i32,
}

// SCROLLS is piecewise constant. Cache its integral once, rather than walking
// the timing map for every visible arrow on every Unity frame.
fn integral(timing: &Timing, beat: f64) -> f64 {
    let mut events: Vec<_> = timing
        .events
        .iter()
        .filter(|e| e.kind == TimingKind::Scrolls && e.beat <= beat)
        .collect();
    events.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let (mut at, mut value, mut position) = (0., 1., 0.);
    for e in events {
        position += (e.beat - at) * value;
        at = e.beat;
        value = e.values[0];
    }
    position + (beat - at) * value
}

// 0 pending, 1 hit, 2 missed, 3 active sustain, 4 completed sustain,
// 5 dropped sustain, 6 mine triggered, 7 nonjudgable/avoided mine.
pub struct Session {
    chart: DanceChart,
    states: Vec<u8>,
    held: [bool; 4],
    last_press: [f64; 4],
    released: [f64; 4],
    speed: f64,
    bot: bool,
    time: f64,
    start: f64,
    end: f64,
    cursor: usize,
    active: Vec<usize>,
    display: Vec<(f64, f64)>,
    snapshot: Snapshot,
    scoring: scoring::Ledger,
    hit_rows: std::collections::HashSet<i64>,
}

impl Session {
    pub fn new(chart: DanceChart, bot: bool, speed: f64, start: f64, end: f64) -> Result<Self> {
        ensure!(
            speed.is_finite() && speed > 0. && speed <= 10.,
            "Invalid song speed"
        );
        ensure!(
            start.is_finite() && end.is_finite() && start < end,
            "Invalid practice range"
        );
        chart.timing.validate()?;
        ensure!(chart.notes.len() <= 2_000_000, "Too many notes");
        for n in &chart.notes {
            ensure!(
                n.lane < 4
                    && n.time.is_finite()
                    && n.end_time.is_finite()
                    && n.beat.is_finite()
                    && n.end_beat.is_finite()
                    && n.end_time >= n.time,
                "Invalid native note"
            );
        }
        ensure!(
            chart.notes.windows(2).all(|w| w[0].time <= w[1].time),
            "Unsorted notes"
        );
        let states: Vec<_> = chart
            .notes
            .iter()
            .map(|n| {
                if !n.judgable || n.time < start || n.time >= end {
                    7
                } else {
                    0
                }
            })
            .collect();
        let total = chart
            .notes
            .iter()
            .zip(&states)
            .filter(|(n, s)| **s == 0 && n.kind != NoteKind::Mine)
            .count() as i32;
        let display = chart
            .notes
            .iter()
            .map(|n| {
                // The exported chart carries beat positions for heads. The tail's
                // position must also keep its original beat through ambiguous warps.
                (
                    integral(&chart.timing, n.beat),
                    integral(&chart.timing, n.end_beat),
                )
            })
            .collect();
        let scoring = scoring::Ledger::new(&chart.notes, bot, start, end);
        Ok(Self {
            chart,
            states,
            held: [false; 4],
            last_press: [f64::NEG_INFINITY; 4],
            released: [f64::NEG_INFINITY; 4],
            speed,
            bot,
            time: f64::NEG_INFINITY,
            start,
            end,
            cursor: 0,
            active: Vec::new(),
            display,
            snapshot: Snapshot {
                total,
                ..Default::default()
            },
            scoring,
            hit_rows: std::collections::HashSet::new(),
        })
    }

    fn feedback(&mut self, judgment: i32, offset: f64) {
        self.snapshot.judgment = judgment;
        self.snapshot.offset = offset;
        self.snapshot.sequence += 1;
    }
    fn hit(&mut self, i: usize, time: f64) {
        let n = &self.chart.notes[i];
        let offset = (time - n.time) / self.speed;
        let sustained = matches!(n.kind, NoteKind::Hold | NoteKind::Roll) && n.end_time > time;
        let judgment = if offset.abs() <= 0.0225 {
            1
        } else if offset.abs() <= 0.045 {
            2
        } else if offset.abs() <= 0.090 {
            3
        } else if offset.abs() <= 0.135 {
            4
        } else {
            5
        };
        self.snapshot.hits += 1;
        if self.hit_rows.insert((n.beat * 48.).round() as i64) {
            self.snapshot.combo += 1;
        }
        self.snapshot.max_combo = self.snapshot.max_combo.max(self.snapshot.combo);
        self.scoring.hit(i, n, time, self.snapshot.combo, self.end);
        self.states[i] = if sustained { 3 } else { 1 };
        if sustained {
            self.released[n.lane as usize] = time;
            self.active.push(i);
        }
        self.feedback(judgment, offset);
    }
    fn miss(&mut self, i: usize) {
        self.states[i] = 2;
        self.snapshot.misses += 1;
        self.snapshot.combo = 0;
        self.scoring.fail(i, self.time);
        self.feedback(6, 0.);
    }
    fn mine(&mut self, i: usize) {
        self.states[i] = 6;
        self.snapshot.mines += 1;
        self.snapshot.combo = 0;
        self.scoring.fail(i, self.time);
        self.feedback(9, 0.);
    }
    pub fn update(&mut self, time: f64) -> Result<Snapshot> {
        ensure!(
            time.is_finite() && time >= self.time,
            "Gameplay time moved backwards; reset before seeking"
        );
        let old = self.time;
        self.time = time;
        let window = WINDOW * self.speed;
        for &i in &self.active {
            let n = &self.chart.notes[i];
            let lane = n.lane as usize;
            let limit = if self.bot {
                f64::INFINITY
            } else if n.kind == NoteKind::Roll {
                self.last_press[lane] + ROLL_GAP * self.speed
            } else if self.held[lane] {
                f64::INFINITY
            } else {
                self.released[lane] + HOLD_GRACE * self.speed
            };
            self.scoring.limit_hold(i, limit);
        }
        let mut i = self.cursor;
        while i < self.chart.notes.len() {
            let n = &self.chart.notes[i];
            if n.time > time + window {
                break;
            }
            if self.states[i] == 0 {
                if n.kind == NoteKind::Mine {
                    if !self.bot
                        && self.held[n.lane as usize]
                        && time >= n.time - MINE_WINDOW * self.speed
                        && old <= n.time + MINE_WINDOW * self.speed
                    {
                        self.mine(i);
                    } else if time > n.time + MINE_WINDOW * self.speed {
                        self.states[i] = 7;
                    }
                } else if self.bot && n.time <= time {
                    self.hit(i, n.time);
                } else if time > n.time + window {
                    self.miss(i);
                }
            }
            i += 1;
        }
        let active = std::mem::take(&mut self.active);
        for i in active {
            let n = &self.chart.notes[i];
            let lane = n.lane as usize;
            let drop_time = if self.bot {
                f64::INFINITY
            } else if n.kind == NoteKind::Roll {
                self.last_press[lane] + ROLL_GAP * self.speed
            } else if self.held[lane] {
                f64::INFINITY
            } else {
                self.released[lane] + HOLD_GRACE * self.speed
            };
            if drop_time < n.end_time.min(self.end) && time > drop_time {
                self.states[i] = 5;
                self.snapshot.drops += 1;
                self.snapshot.combo = 0;
                self.scoring.fail(i, drop_time);
                self.feedback(8, 0.);
            } else if time >= n.end_time.min(self.end) {
                self.states[i] = 4;
                self.snapshot.holds += 1;
                self.feedback(7, 0.);
            } else {
                self.active.push(i);
            }
        }
        while self.cursor < self.states.len() && self.states[self.cursor] != 0 {
            self.cursor += 1;
        }
        self.snapshot.next_note = self.cursor as i32;
        self.scoring.advance(time);
        self.snapshot.score = self.scoring.score();
        Ok(self.snapshot)
    }
    pub fn input(&mut self, time: f64, lane: usize, pressed: bool) -> Result<Snapshot> {
        ensure!(lane < 4, "Invalid input lane");
        self.update(time)?;
        if self.held[lane] == pressed || self.bot {
            return Ok(self.snapshot);
        }
        self.held[lane] = pressed;
        if pressed {
            self.last_press[lane] = time;
        } else {
            self.released[lane] = time;
        }
        let mut candidate = None;
        let mut best = f64::INFINITY;
        for i in self.cursor..self.chart.notes.len() {
            let n = &self.chart.notes[i];
            if n.time > time + WINDOW * self.speed {
                break;
            }
            if self.states[i] != 0 || n.lane as usize != lane {
                continue;
            }
            let distance = ((time - n.time) / self.speed).abs();
            if n.kind == NoteKind::Mine {
                if pressed && distance <= MINE_WINDOW {
                    self.mine(i);
                }
            } else if (pressed != (n.kind == NoteKind::Lift))
                && distance <= WINDOW
                && distance < best
            {
                candidate = Some(i);
                best = distance;
            }
        }
        if let Some(i) = candidate {
            self.hit(i, time);
        }
        self.update(time)
    }
    pub fn distance(&self, i: usize, time: f64) -> f64 {
        let Some(n) = self.chart.notes.get(i) else {
            return 0.;
        };
        let beat = match self.chart.timing.position(time) {
            AudioPosition::Beat(b)
            | AudioPosition::Delay { beat: b }
            | AudioPosition::Stop { beat: b } => b,
            AudioPosition::Warp { to, .. } => to,
        };
        self.chart.timing.display_at(n.beat, beat, time, false)
    }

    pub fn beat(&self, time: f64) -> f64 {
        match self.chart.timing.position(time) {
            AudioPosition::Beat(b)
            | AudioPosition::Delay { beat: b }
            | AudioPosition::Stop { beat: b } => b,
            AudioPosition::Warp { to, .. } => to,
        }
    }
    pub fn render(&self, time: f64, min: f64, max: f64, output: &mut [RenderNote]) -> usize {
        let timing = &self.chart.timing;
        let beat = self.beat(time);
        let origin = integral(timing, beat);
        let mut speeds: Vec<_> = timing
            .events
            .iter()
            .filter(|e| e.kind == TimingKind::Speeds && e.beat <= beat)
            .collect();
        speeds.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let mut speed = 1.;
        for (i, e) in speeds.iter().enumerate() {
            let elapsed = if e.values[2] == 0. {
                beat - e.beat
            } else {
                time - timing.seconds(e.beat, Boundary::Note)
            };
            let previous = if i == 0 { 1. } else { speeds[i - 1].values[0] };
            let ratio = if e.values[1] == 0. {
                1.
            } else {
                (elapsed / e.values[1]).clamp(0., 1.)
            };
            speed = previous + (e.values[0] - previous) * ratio;
        }
        let mut count = 0;
        for (i, n) in self.chart.notes.iter().enumerate() {
            if n.time < self.start_time() || n.time >= self.end {
                continue;
            }
            let state = self.states[i];
            if state != 0 && state != 3 && !(state == 7 && !n.judgable) {
                continue;
            }
            if n.end_time + WINDOW * self.speed < time && state != 3 {
                continue;
            }
            let head = if state == 3 {
                0.
            } else {
                (self.display[i].0 - origin) * speed
            };
            let tail = (self.display[i].1 - origin) * speed;
            if head.max(tail) < min || head.min(tail) > max {
                continue;
            }
            if count == output.len() {
                break;
            }
            output[count] = RenderNote {
                head: head as f32,
                tail: tail as f32,
                lane: n.lane as i32,
                kind: match n.kind {
                    NoteKind::Tap => 0,
                    NoteKind::Hold => 1,
                    NoteKind::Roll => 2,
                    NoteKind::Mine => 3,
                    NoteKind::Lift => 4,
                    NoteKind::Fake => 5,
                },
                state: state as i32,
                index: i as i32,
            };
            count += 1;
        }
        count
    }
    fn start_time(&self) -> f64 {
        self.start
    }
}

fn native_string(f: impl FnOnce() -> Result<String>) -> *mut c_char {
    let result =
        catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| anyhow::bail!("Native dance panic"));
    let text = result.unwrap_or_else(|e| serde_json::json!({"error":format!("{e:#}")}).to_string());
    CString::new(text).expect("JSON contains no NUL").into_raw()
}
unsafe fn utf8<'a>(ptr: *const c_char) -> Result<&'a str> {
    ensure!(!ptr.is_null(), "Null string");
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .context("Invalid UTF-8")
}

#[unsafe(no_mangle)]
pub extern "C" fn yarg_dance_abi() -> u32 {
    1
}

/// Owned UTF-8 noteskin mesh data; release with `yarg_dance_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn yarg_dance_skin() -> *mut c_char {
    catch_unwind(|| {
        CString::new(serde_json::to_string(&skin::create()).unwrap())
            .unwrap()
            .into_raw()
    })
    .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn yarg_dance_color(beat: f64) -> u32 {
    skin::color(beat)
}
#[unsafe(no_mangle)]
pub extern "C" fn yarg_dance_position(distance: f32, note_speed: f32) -> f32 {
    skin::position(distance, note_speed)
}
/// # Safety
/// `ptr` must be null or a live session, exclusively owned by the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yarg_dance_beat(ptr: *const Session, time: f64) -> f64 {
    if ptr.is_null() || !time.is_finite() {
        return 0.;
    }
    unsafe { &*ptr }.beat(time)
}
#[unsafe(no_mangle)]
/// # Safety
/// `text` must be null or point to a readable NUL-terminated string for this call.
/// Release the returned owned string exactly once with `yarg_dance_free_string`.
pub unsafe extern "C" fn yarg_dance_parse(
    text: *const c_char,
    ssc: bool,
    clock_offset: f64,
) -> *mut c_char {
    native_string(|| {
        serde_json::to_string(&parse_for_clock(
            unsafe { utf8(text)? }.into(),
            ssc,
            clock_offset,
        )?)
        .map_err(Into::into)
    })
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or an unreleased string returned by this library.
pub unsafe extern "C" fn yarg_dance_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}
#[unsafe(no_mangle)]
/// # Safety
/// `text` must be null or a readable NUL-terminated string for this call.
/// Destroy a non-null result exactly once, after all session calls have finished.
pub unsafe extern "C" fn yarg_dance_create(
    text: *const c_char,
    bot: bool,
    speed: f64,
    start: f64,
    end: f64,
) -> *mut Session {
    catch_unwind(AssertUnwindSafe(|| -> Result<_> {
        let chart = serde_json::from_str(unsafe { utf8(text)? })?;
        Ok(Box::into_raw(Box::new(Session::new(
            chart, bot, speed, start, end,
        )?)))
    }))
    .ok()
    .and_then(Result::ok)
    .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or an undestroyed handle from `yarg_dance_create`, with no
/// concurrent calls using that handle.
pub unsafe extern "C" fn yarg_dance_destroy(ptr: *mut Session) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session exclusively accessed by this call.
pub unsafe extern "C" fn yarg_dance_set_speed(ptr: *mut Session, speed: f64) -> bool {
    if !speed.is_finite() || speed <= 0. || speed > 10. {
        return false;
    }
    let Some(session) = (unsafe { ptr.as_mut() }) else {
        return false;
    };
    session.speed = speed;
    true
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live, exclusively accessed session. `snapshot` must
/// be null or properly aligned writable storage for one `Snapshot`.
pub unsafe extern "C" fn yarg_dance_update(
    ptr: *mut Session,
    time: f64,
    lane: i32,
    pressed: bool,
    snapshot: *mut Snapshot,
) -> bool {
    if ptr.is_null() || snapshot.is_null() {
        return false;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let session = unsafe { &mut *ptr };
        let result = if lane == 4 {
            session.update(time).map(|_| {
                session.scoring.power_input(time, pressed);
                session.snapshot
            })
        } else if lane < 0 {
            session.update(time)
        } else {
            session.input(time, lane as usize, pressed)
        };
        match result {
            Ok(s) => {
                unsafe { *snapshot = s };
                true
            }
            Err(_) => false,
        }
    }))
    .unwrap_or(false)
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session that is not concurrently mutated.
pub unsafe extern "C" fn yarg_dance_state(ptr: *const Session, note: usize) -> u8 {
    unsafe { ptr.as_ref() }
        .and_then(|s| s.states.get(note))
        .copied()
        .unwrap_or(7)
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session that is not concurrently mutated.
pub unsafe extern "C" fn yarg_dance_distance(ptr: *const Session, note: usize, time: f64) -> f64 {
    if !time.is_finite() {
        return 0.;
    }
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { ptr.as_ref() }.map_or(0., |s| s.distance(note, time))
    }))
    .unwrap_or(0.)
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session that is not concurrently mutated.
/// `output` must be null or an aligned, writable array of `capacity` elements.
pub unsafe extern "C" fn yarg_dance_render(
    ptr: *const Session,
    time: f64,
    min: f64,
    max: f64,
    output: *mut RenderNote,
    capacity: usize,
) -> usize {
    if ptr.is_null()
        || output.is_null()
        || capacity > 4096
        || !time.is_finite()
        || !min.is_finite()
        || !max.is_finite()
    {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { &*ptr }.render(time, min, max, unsafe {
            std::slice::from_raw_parts_mut(output, capacity)
        })
    }))
    .unwrap_or(0)
}

#[cfg(test)]
mod scoring_tests;
#[cfg(test)]
mod tests;

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be a live exclusively accessed session and `json` readable UTF-8.
pub unsafe extern "C" fn yarg_dance_configure_score(
    ptr: *mut Session,
    json: *const c_char,
) -> bool {
    if ptr.is_null() {
        return false;
    }
    catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        let config = serde_json::from_str(unsafe { utf8(json)? })?;
        let s = unsafe { &mut *ptr };
        s.scoring.configure(config, &s.chart.notes, s.start, s.end)
    }))
    .is_ok_and(|r| r.is_ok())
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session; output must be null or aligned writable
/// storage for one ScoreState, without concurrent access.
pub unsafe extern "C" fn yarg_dance_score(
    ptr: *const Session,
    output: *mut scoring::ScoreState,
) -> bool {
    if ptr.is_null() || output.is_null() {
        return false;
    }
    unsafe {
        *output = (&*ptr).scoring.state();
    }
    true
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live exclusively accessed session.
pub unsafe extern "C" fn yarg_dance_score_context(ptr: *mut Session, band: i32, allowed: bool) {
    if let Some(s) = unsafe { ptr.as_mut() } {
        s.scoring.context(band, allowed);
    }
}
#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be null or a live session that is not concurrently mutated.
pub unsafe extern "C" fn yarg_dance_power_note(ptr: *const Session, index: usize) -> bool {
    unsafe { ptr.as_ref() }.is_some_and(|s| s.scoring.is_phrase(index))
}
