use super::*;

fn chart(notes: &[(f64, u8, NoteKind, f64)]) -> DanceChart {
    DanceChart {
        name: "Fixture".into(),
        difficulty: 4,
        meter: 10,
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
fn session(notes: &[(f64, u8, NoteKind, f64)]) -> Session {
    Session::new(chart(notes), false, 1., -1e10, 1e10).unwrap()
}

#[test]
fn simultaneous_steps_require_independent_edges_and_holding_does_not_repeat() {
    let mut s = session(&[
        (1., 0, NoteKind::Tap, 1.),
        (1., 3, NoteKind::Tap, 1.),
        (1.5, 0, NoteKind::Tap, 1.5),
    ]);
    s.input(1., 0, true).unwrap();
    s.input(1., 3, true).unwrap();
    assert_eq!(s.snapshot.hits, 2);
    s.input(1.5, 0, true).unwrap();
    s.update(1.7).unwrap();
    assert_eq!((s.snapshot.hits, s.snapshot.misses), (2, 1));
}
#[test]
fn early_press_can_be_released_before_a_later_note() {
    let mut s = session(&[(1., 0, NoteKind::Tap, 1.)]);
    s.input(0.5, 0, true).unwrap();
    s.input(0.6, 0, false).unwrap();
    s.input(1., 0, true).unwrap();
    assert_eq!(s.snapshot.hits, 1);
}
#[test]
fn lift_requires_release_and_fake_is_never_scored() {
    let mut s = session(&[(1., 0, NoteKind::Lift, 1.), (2., 1, NoteKind::Fake, 2.)]);
    s.input(0.9, 0, true).unwrap();
    assert_eq!(s.snapshot.hits, 0);
    s.input(1., 0, false).unwrap();
    s.input(2., 1, true).unwrap();
    s.update(3.).unwrap();
    assert_eq!(
        (s.snapshot.hits, s.snapshot.total, s.snapshot.misses),
        (1, 1, 0)
    );
}
#[test]
fn holds_finish_or_drop_even_with_sparse_updates() {
    let mut held = session(&[(1., 0, NoteKind::Hold, 2.)]);
    held.input(1., 0, true).unwrap();
    held.update(3.).unwrap();
    assert_eq!((held.states[0], held.snapshot.holds), (4, 1));
    let mut dropped = session(&[(1., 0, NoteKind::Hold, 2.)]);
    dropped.input(1., 0, true).unwrap();
    dropped.input(1.2, 0, false).unwrap();
    dropped.update(3.).unwrap();
    assert_eq!((dropped.states[0], dropped.snapshot.drops), (5, 1));
}
#[test]
fn release_grace_allows_pad_contact_bounce() {
    let mut s = session(&[(1., 0, NoteKind::Hold, 2.)]);
    s.input(1., 0, true).unwrap();
    s.input(1.3, 0, false).unwrap();
    s.input(1.4, 0, true).unwrap();
    s.update(2.).unwrap();
    assert_eq!(s.snapshot.holds, 1);
}
#[test]
fn rolls_require_repeated_presses() {
    let mut s = session(&[(1., 0, NoteKind::Roll, 2.)]);
    s.input(1., 0, true).unwrap();
    s.update(1.6).unwrap();
    assert_eq!(s.snapshot.drops, 1);
    let mut s = session(&[(1., 0, NoteKind::Roll, 2.)]);
    s.input(1., 0, true).unwrap();
    for t in [1.3, 1.6, 1.9] {
        s.input(t, 0, false).unwrap();
        s.input(t, 0, true).unwrap();
    }
    s.update(2.).unwrap();
    assert_eq!(s.snapshot.holds, 1);
}
#[test]
fn mines_trigger_when_held_across_a_skipped_frame_but_not_when_avoided() {
    let mut s = session(&[(1., 0, NoteKind::Mine, 1.)]);
    s.input(0.5, 0, true).unwrap();
    s.update(1.5).unwrap();
    assert_eq!(s.snapshot.mines, 1);
    let mut s = session(&[(1., 0, NoteKind::Mine, 1.)]);
    s.input(0.5, 0, true).unwrap();
    s.input(0.8, 0, false).unwrap();
    s.update(2.).unwrap();
    assert_eq!(s.snapshot.mines, 0);
}
#[test]
fn speed_scales_windows_in_recording_time() {
    let mut s = Session::new(chart(&[(1., 0, NoteKind::Tap, 1.)]), false, 2., -1e10, 1e10).unwrap();
    s.input(1.3, 0, true).unwrap();
    assert_eq!(s.snapshot.hits, 1);
    assert!((s.snapshot.offset - 0.15).abs() < 1e-9);
}
#[test]
fn practice_filters_notes_and_clips_holds() {
    let mut s = Session::new(
        chart(&[
            (1., 0, NoteKind::Tap, 1.),
            (2., 0, NoteKind::Hold, 4.),
            (3., 1, NoteKind::Tap, 3.),
        ]),
        false,
        1.,
        2.,
        3.,
    )
    .unwrap();
    assert_eq!(s.snapshot.total, 1);
    s.input(2., 0, true).unwrap();
    s.update(3.).unwrap();
    assert_eq!(
        (s.snapshot.hits, s.snapshot.holds, s.snapshot.misses),
        (1, 1, 0)
    );
}
#[test]
fn bot_is_frame_rate_independent_and_avoids_mines() {
    let c = chart(&[
        (1., 0, NoteKind::Hold, 2.),
        (1.5, 1, NoteKind::Tap, 1.5),
        (2., 2, NoteKind::Mine, 2.),
    ]);
    let mut coarse = Session::new(c.clone(), true, 1., -1e10, 1e10).unwrap();
    coarse.update(4.).unwrap();
    let mut fine = Session::new(c, true, 1., -1e10, 1e10).unwrap();
    for i in 0..400 {
        fine.update(i as f64 / 100.).unwrap();
    }
    assert_eq!(
        (
            coarse.snapshot.score,
            coarse.snapshot.hits,
            coarse.snapshot.holds
        ),
        (fine.snapshot.score, fine.snapshot.hits, fine.snapshot.holds)
    );
    assert_eq!(coarse.snapshot.mines, 0);
}
#[test]
fn reset_is_explicit_and_malformed_ffi_input_fails_safely() {
    let mut s = session(&[]);
    s.update(1.).unwrap();
    assert!(s.update(0.).is_err());
    assert!(s.update(f64::NAN).is_err());
    assert!(s.input(2., 4, true).is_err());
    unsafe {
        let bad = CString::new("{}").unwrap();
        assert!(yarg_dance_create(bad.as_ptr(), false, 1., 0., 10.).is_null());
        assert!(!yarg_dance_update(
            std::ptr::null_mut(),
            1.,
            -1,
            false,
            std::ptr::null_mut()
        ));
        let result = yarg_dance_parse(std::ptr::null(), true, 0.);
        assert!(CStr::from_ptr(result).to_str().unwrap().contains("error"));
        yarg_dance_free_string(result);
    }
}

const SSC: &str = "#VERSION:0.83;\n#TITLE:Mixed band;\n#ARTIST:YARG fixture;\n#MUSIC:song.ogg;\n#OFFSET:0.25;\n#BPMS:0=120;\n#STOPS:2=1;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DESCRIPTION:Pad;\n#DIFFICULTY:Hard;\n#METER:5;\n#NOTES:\n1000\n0100\n0010\n0001\n;\n";
#[test]
fn ssc_preserves_offset_and_stop_and_does_not_change_band_clock() {
    let parsed = parse(SSC.into(), true).unwrap();
    let c = &parsed.charts[0];
    assert_eq!(c.difficulty, 3);
    assert_eq!(
        c.notes.iter().map(|n| n.time).collect::<Vec<_>>(),
        vec![-0.25, 0.25, 0.75, 2.25]
    );
    assert_eq!(c.notes[3].lane, 3);
    // A band note at beat 3 remains 1.5 seconds; no shared tempo-map mutation.
    assert_eq!(Timing::default().seconds(3., Boundary::Note), 1.5);
}
#[test]
fn chart_timing_overrides_song_timing() {
    let text = SSC.replace("#METER:5;", "#METER:5;\n#OFFSET:0;\n#BPMS:0=60;\n#STOPS:;");
    let parsed = parse(text, true).unwrap();
    assert_eq!(parsed.charts[0].notes[3].time, 3.);
}
#[test]
fn standalone_sm_and_ssc_produce_matching_basic_charts() {
    let sm = "#TITLE:SM;#MUSIC:song.ogg;#BPMS:0=120;#NOTES:dance-single:Keyboard:Challenge:9:0,0,0,0,0:\n1000\n0100\n0010\n0001\n;";
    let parsed = parse(sm.into(), false).unwrap();
    assert_eq!(parsed.charts[0].difficulty, 4);
    assert_eq!(parsed.charts[0].notes[3].time, 1.5);
}
#[test]
fn ffi_snapshot_layout_is_stable() {
    assert_eq!(std::mem::size_of::<Snapshot>(), 56);
    assert_eq!(std::mem::offset_of!(Snapshot, offset), 48);
    assert_eq!(yarg_dance_abi(), 1);
}

#[test]
fn batched_render_matches_yace_for_stops_reverse_scroll_and_speed_transitions() {
    let text = SSC.replace(
        "#STOPS:2=1;",
        "#STOPS:2=1;\n#SCROLLS:0=1,1=0,2=-1,3=2;\n#SPEEDS:0=1=0=0,2=0.5=1=1;",
    );
    let song = parse(text, true).unwrap();
    let session = Session::new(song.charts[0].clone(), false, 1., -1e10, 1e10).unwrap();
    let mut output = [RenderNote::default(); 64];
    for time in [-0.3, 0., 0.5, 0.8, 1.25, 2., 2.3] {
        let count = session.render(time, -100., 100., &mut output);
        for rendered in &output[..count] {
            let expected = session.distance(rendered.index as usize, time);
            assert!((rendered.head as f64 - expected).abs() < 1e-5);
        }
    }
    assert_eq!(std::mem::size_of::<RenderNote>(), 24);
}

#[test]
fn shifted_split_timing_preserves_recording_sync() {
    let text = SSC.replace(
        "#METER:5;",
        "#METER:5;\n#OFFSET:0.5;\n#BPMS:0=60;\n#STOPS:;",
    );
    let original = parse(text.clone(), true).unwrap();
    let shifted = parse_for_clock(text, true, 0.75).unwrap();
    for (before, after) in original.charts[0]
        .notes
        .iter()
        .zip(&shifted.charts[0].notes)
    {
        assert!((before.time - 0.75 - after.time).abs() < 1e-9);
    }
}

#[test]
fn warp_and_fake_sections_are_visible_but_never_judged() {
    let text = SSC.replace("#STOPS:2=1;", "#WARPS:1=1;\n#FAKES:3=1;");
    let song = parse(text, true).unwrap();
    let chart = song.charts[0].clone();
    assert!(chart.notes.iter().any(|n| !n.judgable));
    let total = chart.notes.iter().filter(|n| n.judgable).count() as i32;
    let mut bot = Session::new(chart, true, 1., -1e10, 1e10).unwrap();
    bot.update(10.).unwrap();
    assert_eq!(bot.snapshot.hits, total);
    assert_eq!(bot.snapshot.misses, 0);
}
