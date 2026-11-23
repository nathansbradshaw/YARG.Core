use std::{f64::consts::TAU, slice};

const SAMPLE_RATE: usize = 24_000;
const COUNT_IN: usize = 4;
const MEASURED_BEATS: usize = 16;
const BEAT_FRAMES: usize = SAMPLE_RATE * 3 / 5;
const FIRST_FRAME: usize = SAMPLE_RATE / 2;
const CLICK_FRAMES: usize = SAMPLE_RATE / 25;
const TRACK_FRAMES: usize = FIRST_FRAME + (COUNT_IN + MEASURED_BEATS) * BEAT_FRAMES;
const WAV_SIZE: usize = 44 + TRACK_FRAMES * 2;

fn offset_ms(hits: &[f64]) -> Option<i32> {
    const BEAT: f64 = BEAT_FRAMES as f64 / SAMPLE_RATE as f64;
    const FIRST_MEASURED: f64 = FIRST_FRAME as f64 / SAMPLE_RATE as f64 + COUNT_IN as f64 * BEAT;
    const MIN_HITS: usize = 10;

    let mut beats: [Option<f64>; MEASURED_BEATS] = [None; MEASURED_BEATS];
    for &time in hits {
        let relative = time - FIRST_MEASURED;
        let beat = (relative / BEAT).round();
        let error = relative - beat * BEAT;
        if !((0.0..MEASURED_BEATS as f64).contains(&beat) && error.abs() <= 0.23) {
            continue;
        }
        let entry = &mut beats[beat as usize];
        if entry.is_none_or(|other| error < other) {
            *entry = Some(error);
        }
    }

    let mut offsets: Vec<f64> = beats.into_iter().flatten().collect();
    if offsets.len() < MIN_HITS {
        return None;
    }
    offsets.sort_by(f64::total_cmp);
    let center = median(&offsets);
    offsets.retain(|offset| (*offset - center).abs() <= 0.045);
    (offsets.len() >= MIN_HITS).then(|| (median(&offsets) * 1000.0).round() as i32)
}

fn median(sorted: &[f64]) -> f64 {
    (sorted[(sorted.len() - 1) / 2] + sorted[sorted.len() / 2]) / 2.0
}

fn write_click_track(output: &mut [u8]) {
    output.fill(0);
    output[0..4].copy_from_slice(b"RIFF");
    output[4..8].copy_from_slice(&((WAV_SIZE - 8) as u32).to_le_bytes());
    output[8..16].copy_from_slice(b"WAVEfmt ");
    output[16..20].copy_from_slice(&16u32.to_le_bytes());
    output[20..24].copy_from_slice(&[1, 0, 1, 0]); // mono PCM
    output[24..28].copy_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    output[28..32].copy_from_slice(&((SAMPLE_RATE * 2) as u32).to_le_bytes());
    output[32..36].copy_from_slice(&[2, 0, 16, 0]); // 16 bits per frame
    output[36..40].copy_from_slice(b"data");
    output[40..44].copy_from_slice(&((TRACK_FRAMES * 2) as u32).to_le_bytes());

    for beat in 0..COUNT_IN + MEASURED_BEATS {
        let frequency = if beat < COUNT_IN { 650.0 } else { 1_350.0 };
        for sample in 0..CLICK_FRAMES {
            let cycle = frequency * sample as f64 / SAMPLE_RATE as f64;
            let envelope = (1.0 - sample as f64 / CLICK_FRAMES as f64).powi(3);
            let value = ((TAU * cycle).cos() * envelope * 16_000.0) as i16;
            let index = 44 + (FIRST_FRAME + beat * BEAT_FRAMES + sample) * 2;
            output[index..index + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
}

/// The caller may query the required capacity with a null pointer.
///
/// # Safety
/// A non-null buffer must be valid and writable for the declared capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yarg_input_calibration_clicks(output: *mut u8, capacity: usize) -> usize {
    if !output.is_null() && capacity >= WAV_SIZE {
        write_click_track(unsafe { slice::from_raw_parts_mut(output, WAV_SIZE) });
    }
    WAV_SIZE
}

/// Hit times are in gameplay-clock seconds. i32::MIN means the run needs more consistent hits.
///
/// # Safety
/// A non-null pointer must reference a valid array of the declared length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yarg_input_calibration_offset(hits: *const f64, count: usize) -> i32 {
    if hits.is_null() || count == 0 || count > 256 {
        return i32::MIN;
    }
    offset_ms(unsafe { slice::from_raw_parts(hits, count) }).unwrap_or(i32::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(i: usize) -> f64 {
        (FIRST_FRAME + (COUNT_IN + i) * BEAT_FRAMES) as f64 / SAMPLE_RATE as f64
    }

    #[test]
    fn finds_signed_offsets_despite_misses_and_jitter() {
        for ms in [-72, 0, 83] {
            let hits: Vec<_> = (0..MEASURED_BEATS)
                .filter(|i| *i != 3 && *i != 7)
                .map(|i| beat(i) + (ms + (i as i32 % 3 - 1) * 3) as f64 / 1000.0)
                .collect();
            assert_eq!(offset_ms(&hits), Some(ms));
        }
    }

    #[test]
    fn ignores_count_in_repeats_and_outliers() {
        let mut hits = vec![0.5, 1.1, 1.7, 2.3, f64::NAN, f64::INFINITY];
        for i in 0..MEASURED_BEATS {
            hits.extend([beat(i) + if i < 3 { 0.19 } else { 0.07 }, beat(i) + 0.21]);
        }
        assert_eq!(offset_ms(&hits), Some(70));
    }

    #[test]
    fn rejects_sparse_and_inconsistent_hits() {
        let single_beat = vec![beat(3); 40];
        let inconsistent: Vec<_> = (0..16)
            .map(|i| beat(i) + if i % 2 == 0 { -0.085 } else { 0.09 })
            .collect();
        assert_eq!(offset_ms(&single_beat), None);
        assert_eq!(offset_ms(&inconsistent), None);
    }

    #[test]
    fn audible_clicks_line_up_with_the_estimator() {
        let mut wave = vec![0; WAV_SIZE];
        write_click_track(&mut wave);
        assert_eq!(&wave[0..4], b"RIFF");
        for i in 0..MEASURED_BEATS {
            let index = 44 + (beat(i) * SAMPLE_RATE as f64).round() as usize * 2;
            assert_eq!(&wave[index - 2..index], &[0, 0]);
            assert_eq!(&wave[index..index + 2], &16_000i16.to_le_bytes());
        }
    }
}
