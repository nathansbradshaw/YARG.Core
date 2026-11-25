use crossbeam_queue::ArrayQueue;
use std::{
    mem::MaybeUninit,
    ptr, slice,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_BUFFER_MS: usize = 24;
const PITCH_BUFFER_MS: usize = 120;
const START_MARGIN_MS: usize = 4;

struct MonitorQueue {
    samples: ArrayQueue<i16>,
    start_margin: usize,
    primed: AtomicBool,
}

impl MonitorQueue {
    fn new(sample_rate: usize, buffer_ms: usize) -> Self {
        Self {
            samples: ArrayQueue::new(sample_rate * buffer_ms / 1000),
            start_margin: sample_rate * START_MARGIN_MS / 1000,
            primed: AtomicBool::new(false),
        }
    }

    fn push(&self, samples: &[i16]) {
        let newest = samples.len().saturating_sub(self.samples.capacity());
        for sample in &samples[newest..] {
            self.samples.force_push(*sample);
        }
    }

    fn render(&self, output: &mut [MaybeUninit<i16>]) {
        if !self.primed.load(Ordering::Relaxed) {
            let threshold = (output.len() + self.start_margin).min(self.samples.capacity());
            if self.samples.len() < threshold {
                output.fill(MaybeUninit::new(0));
                return;
            }
            self.primed.store(true, Ordering::Relaxed);
        }

        for index in 0..output.len() {
            match self.samples.pop() {
                Some(sample) => {
                    output[index].write(sample);
                }
                None => {
                    output[index..].fill(MaybeUninit::new(0));
                    self.primed.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    fn clear(&self) {
        for _ in 0..self.samples.len() {
            self.samples.pop();
        }
        self.primed.store(false, Ordering::Relaxed);
    }

    fn read_pitch_block(&self, output: &mut [MaybeUninit<i16>]) -> usize {
        if self.samples.len() < output.len() {
            return 0;
        }
        for (index, destination) in output.iter_mut().enumerate() {
            let Some(sample) = self.samples.pop() else {
                return index;
            };
            destination.write(sample);
        }
        output.len()
    }
}

#[unsafe(no_mangle)]
extern "C" fn yarg_mic_monitor_create(sample_rate: u32) -> *mut MonitorQueue {
    create_queue(sample_rate, MAX_BUFFER_MS)
}

#[unsafe(no_mangle)]
extern "C" fn yarg_mic_pitch_create(sample_rate: u32) -> *mut MonitorQueue {
    create_queue(sample_rate, PITCH_BUFFER_MS)
}

fn create_queue(sample_rate: u32, buffer_ms: usize) -> *mut MonitorQueue {
    if !(8_000..=192_000).contains(&sample_rate) {
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(MonitorQueue::new(sample_rate as usize, buffer_ms)))
}

// The host stops both BASS callbacks before destroying this handle.
#[unsafe(no_mangle)]
unsafe extern "C" fn yarg_mic_monitor_destroy(queue: *mut MonitorQueue) {
    if !queue.is_null() {
        drop(unsafe { Box::from_raw(queue) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn yarg_mic_monitor_push(
    queue: *const MonitorQueue,
    pcm: *const i16,
    samples: usize,
) {
    if let Some(queue) = unsafe { queue.as_ref() } {
        if !pcm.is_null() && samples > 0 {
            queue.push(unsafe { slice::from_raw_parts(pcm, samples) });
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn yarg_mic_monitor_render(
    queue: *const MonitorQueue,
    pcm: *mut MaybeUninit<i16>,
    samples: usize,
) {
    if let Some(queue) = unsafe { queue.as_ref() } {
        if !pcm.is_null() && samples > 0 {
            queue.render(unsafe { slice::from_raw_parts_mut(pcm, samples) });
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn yarg_mic_monitor_clear(queue: *const MonitorQueue) {
    if let Some(queue) = unsafe { queue.as_ref() } {
        queue.clear();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn yarg_mic_pitch_read(
    queue: *const MonitorQueue,
    pcm: *mut MaybeUninit<i16>,
    samples: usize,
) -> usize {
    if let Some(queue) = unsafe { queue.as_ref() } {
        if !pcm.is_null() && samples > 0 {
            return queue.read_pitch_block(unsafe { slice::from_raw_parts_mut(pcm, samples) });
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render<const N: usize>(queue: &MonitorQueue) -> [i16; N] {
        let mut output = [MaybeUninit::uninit(); N];
        queue.render(&mut output);
        output.map(|sample| unsafe { sample.assume_init() })
    }

    #[test]
    fn monitor_preserves_full_scale_pcm() {
        let queue = MonitorQueue::new(1_000, MAX_BUFFER_MS);
        queue.push(&[i16::MIN, -512, 17, i16::MAX, 0, 1, 2, 3]);
        assert_eq!(render(&queue), [i16::MIN, -512, 17, i16::MAX]);
        assert_eq!(render(&queue), [0, 1, 2, 3]);
    }

    #[test]
    fn stall_only_keeps_the_latest_samples() {
        let queue = MonitorQueue::new(1_000, MAX_BUFFER_MS);
        queue.push(&(0..120).collect::<Vec<_>>());
        assert_eq!(queue.samples.len(), 24);
        assert_eq!(render(&queue), [96, 97, 98, 99]);
    }

    #[test]
    fn underrun_renders_silence_and_waits_for_new_audio() {
        let queue = MonitorQueue::new(1_000, MAX_BUFFER_MS);
        queue.push(&[1; 9]);
        assert_eq!(render(&queue), [1; 4]);
        assert_eq!(render(&queue), [1; 4]);
        assert_eq!(render(&queue), [1, 0, 0, 0]);
        queue.push(&[2; 4]);
        assert_eq!(render(&queue), [0; 4]);
        queue.push(&[3; 4]);
        assert_eq!(render(&queue), [2; 4]);
        queue.clear();
        assert_eq!(render(&queue), [0; 4]);
    }

    #[test]
    fn a_stalled_pitch_consumer_does_not_hold_up_the_monitor() {
        let monitor = MonitorQueue::new(1_000, MAX_BUFFER_MS);
        let pitch = MonitorQueue::new(1_000, PITCH_BUFFER_MS);
        for value in 1..=40 {
            let block = [value; 5];
            monitor.push(&block);
            pitch.push(&block);
        }

        assert_eq!(render(&monitor), [36, 36, 36, 36]);
        assert_eq!(pitch.samples.len(), 120);
        let mut window = [MaybeUninit::uninit(); 40];
        assert_eq!(pitch.read_pitch_block(&mut window), 40);
        let samples = window.map(|sample| unsafe { sample.assume_init() });
        assert_eq!((samples[0], samples[39]), (17, 24));
        pitch.clear();
        assert_eq!(pitch.read_pitch_block(&mut window), 0);
    }
}
