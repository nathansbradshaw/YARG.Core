use super::*;
use scoring::{ClockPoint, Config, PhraseRange};

fn chart(notes: &[(f64, u8, NoteKind, f64)]) -> DanceChart {
    DanceChart {
        name: "Score test".into(),
        difficulty: 4,
        meter: 8,
        timing: Timing::default(),
        notes: notes
            .iter()
            .map(|&(time, lane, kind, end_time)| DanceNote {
                time,
                lane,
                kind,
                end_time,
                beat: time * 2.,
                end_beat: end_time * 2.,
                judgable: kind != NoteKind::Fake,
            })
            .collect(),
    }
}
fn session(notes: &[(f64, u8, NoteKind, f64)], bot: bool) -> Session {
    Session::new(chart(notes), bot, 1., -1e10, 1e10).unwrap()
}
fn clock() -> ClockPoint {
    ClockPoint {
        time: 0.,
        beat: 0.,
        measure: 0.,
        beats_per_second: 2.,
        measures_per_second: 0.5,
    }
}
fn config(phrases: Vec<PhraseRange>) -> Config {
    Config {
        clock: vec![clock()],
        phrases,
    }
}

#[test]
fn half_point_sustain_rounding_is_independent_of_frame_boundaries() {
    let notes = [(1., 0, NoteKind::Hold, 2.25)];
    for fps in [1., 59., 60., 144., 240.] {
        let mut s = session(&notes, true);
        for frame in 0..(4. * fps) as usize {
            s.update(frame as f64 / fps).unwrap();
        }
        assert_eq!(s.scoring.state().sustain, 63, "fps={fps}");
    }
}
#[test]
fn fifty_point_heads_and_chord_combo_ramp() {
    let notes: Vec<_> = (1..=40)
        .flat_map(|i| {
            [
                (i as f64, 0, NoteKind::Tap, i as f64),
                (i as f64, 1, NoteKind::Tap, i as f64),
            ]
        })
        .collect();
    let mut s = session(&notes, true);
    s.scoring.context(1, false);
    s.update(45.).unwrap();
    let score = s.scoring.state();
    assert_eq!(
        (s.snapshot.hits, s.snapshot.combo, score.total_chords),
        (80, 40, 40)
    );
    assert_eq!(
        (score.note, score.score_multiplier, s.snapshot.score),
        (4000, 4, 10300)
    );
    assert_eq!(score.phrases_hit, 0);
}
#[test]
fn sustains_earn_twenty_five_per_beat_and_no_completion_bounty() {
    let notes = [(1., 0, NoteKind::Hold, 3.)];
    let mut full = session(&notes, false);
    full.input(1., 0, true).unwrap();
    full.update(3.).unwrap();
    assert_eq!(
        (full.scoring.state().sustain, full.snapshot.score),
        (100, 150)
    );
    let mut drop = session(&notes, false);
    drop.input(1., 0, true).unwrap();
    drop.input(1.5, 0, false).unwrap();
    drop.update(10.).unwrap();
    assert_eq!((drop.snapshot.drops, drop.snapshot.score), (1, 88));
    drop.update(20.).unwrap();
    assert_eq!(drop.snapshot.score, 88);
    let mut bounce = session(&notes, false);
    bounce.input(1., 0, true).unwrap();
    bounce.input(1.5, 0, false).unwrap();
    bounce.input(1.6, 0, true).unwrap();
    bounce.update(3.).unwrap();
    assert_eq!(bounce.snapshot.score, 150);
}
#[test]
fn grades_do_not_reduce_valid_hit_points_and_failed_phrase_gives_no_power() {
    let mut s = session(
        &[
            (1., 0, NoteKind::Tap, 1.),
            (2., 1, NoteKind::Tap, 2.),
            (5., 2, NoteKind::Tap, 5.),
            (6., 3, NoteKind::Tap, 6.),
        ],
        false,
    );
    s.scoring
        .configure(
            config(vec![
                PhraseRange { start: 1., end: 3. },
                PhraseRange { start: 5., end: 7. },
            ]),
            &s.chart.notes,
            s.start,
            s.end,
        )
        .unwrap();
    s.input(1.16, 0, true).unwrap();
    assert_eq!(s.snapshot.score, 50);
    s.input(2., 1, true).unwrap();
    assert_eq!(s.scoring.state().power, 0.25);
    s.update(5.5).unwrap();
    s.input(6., 3, true).unwrap();
    s.scoring.power_input(6., true);
    let score = s.scoring.state();
    assert_eq!(
        (score.phrases_hit, score.phrases_missed, score.active),
        (1, 1, 0)
    );
    assert_eq!(score.power, 0.25);
}
#[test]
fn generated_phrases_auto_activate_and_drain_independently_of_frame_rate() {
    let notes: Vec<_> = (0..64)
        .map(|i| (i as f64, i as u8 % 4, NoteKind::Tap, i as f64))
        .collect();
    let mut coarse = session(&notes, true);
    coarse.update(30.).unwrap();
    let mut fine = session(&notes, true);
    for i in 0..=3600 {
        fine.update(i as f64 / 120.).unwrap();
    }
    let score = coarse.scoring.state();
    assert_eq!(
        (score.phrases_hit, score.activations, score.active),
        (2, 1, 0)
    );
    assert!((score.power_time - 8.).abs() < 1e-7);
    assert_eq!(coarse.snapshot.score, fine.snapshot.score);
    assert_eq!(
        score.star_power_score,
        fine.scoring.state().star_power_score
    );
}
#[test]
fn power_drains_in_band_measures_across_tempo_and_signature_changes() {
    let mut s = session(
        &[(0., 0, NoteKind::Tap, 0.), (1., 1, NoteKind::Tap, 1.)],
        true,
    );
    let c = Config {
        clock: vec![
            clock(),
            ClockPoint {
                time: 5.,
                beat: 10.,
                measure: 2.5,
                beats_per_second: 1.,
                measures_per_second: 1. / 3.,
            },
        ],
        phrases: vec![
            PhraseRange {
                start: 0.,
                end: 0.5,
            },
            PhraseRange {
                start: 1.,
                end: 1.5,
            },
        ],
    };
    s.scoring
        .configure(c, &s.chart.notes, s.start, s.end)
        .unwrap();
    s.update(12.).unwrap();
    let score = s.scoring.state();
    assert_eq!((score.activations, score.active), (1, 0));
    assert!((score.power_time - 10.).abs() < 1e-6);
}
#[test]
fn band_bonus_is_separate_from_personal_score() {
    let mut s = session(
        &[(1., 0, NoteKind::Tap, 1.), (2., 1, NoteKind::Tap, 2.)],
        false,
    );
    s.scoring.context(4, false);
    s.input(1., 0, true).unwrap();
    s.input(2., 1, true).unwrap();
    let score = s.scoring.state();
    assert_eq!(
        (s.snapshot.score, score.note, score.band_bonus),
        (100, 100, 300)
    );
    assert_eq!(std::mem::size_of::<scoring::ScoreState>(), 88);
}

#[test]
fn mine_breaks_power_phrase_without_subtracting_banked_points() {
    let mut s = session(
        &[
            (1., 0, NoteKind::Tap, 1.),
            (1.5, 1, NoteKind::Mine, 1.5),
            (2., 2, NoteKind::Tap, 2.),
        ],
        false,
    );
    s.input(1., 0, true).unwrap();
    s.input(1.5, 1, true).unwrap();
    s.input(2., 2, true).unwrap();
    let score = s.scoring.state();
    assert_eq!(
        (score.phrases_hit, score.phrases_missed, s.snapshot.score),
        (0, 1, 100)
    );
}

#[test]
fn active_hold_is_rebased_when_power_starts_and_expires() {
    let mut s = session(
        &[(0., 0, NoteKind::Tap, 0.), (1., 1, NoteKind::Hold, 11.)],
        true,
    );
    s.scoring
        .configure(
            config(vec![
                PhraseRange {
                    start: 0.,
                    end: 0.5,
                },
                PhraseRange {
                    start: 1.,
                    end: 1.5,
                },
            ]),
            &s.chart.notes,
            s.start,
            s.end,
        )
        .unwrap();
    s.update(12.).unwrap();
    let score = s.scoring.state();
    assert_eq!(
        (score.sustain, score.star_power_score, s.snapshot.score),
        (500, 400, 1000)
    );
}
