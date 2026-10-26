//! A stand-in for the system's audio output, for tests: it plays nothing,
//! so tests make no noise, and records what each ringing gave it, so they
//! can check that a sound played, how much of it, how loud, and that it
//! stopped.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::{OpenError, Output, Sink, lock};

/// The stand-in output. Shared: the runtime plays through it, the test
/// reads it (see `Runtime::use_stand_in_sound`).
#[derive(Default)]
pub struct StandInSound {
    /// Whether there is no output device: opening answers so.
    no_device: AtomicBool,
    /// How many frames a stream takes at each look; 0 is the default,
    /// 4,800 (ten times as fast as a 48 kHz sound plays).
    speed: AtomicUsize,
    streams: Mutex<Vec<Arc<Stream>>>,
}

/// What one ringing gave the stand-in.
#[derive(Default)]
struct Stream {
    rate: AtomicU64,
    frames: AtomicU64,
    /// The loudest sample, as `f32` bits.
    peak: AtomicU64,
    open: AtomicBool,
    /// Set to make the stream say its device went away at its next look.
    lost: AtomicBool,
}

impl StandInSound {
    pub fn new() -> Arc<StandInSound> {
        Arc::default()
    }

    /// Answers from now on that there is no output device, or that there
    /// is one again.
    pub fn set_device(&self, present: bool) {
        self.no_device.store(!present, Ordering::SeqCst);
    }

    /// Has every stream take `frames` at each look from now on.
    pub fn set_speed(&self, frames: usize) {
        self.speed.store(frames, Ordering::SeqCst);
    }

    /// The device goes away under every stream open now, as when it is
    /// unplugged mid-ring.
    pub fn lose_device(&self) {
        for stream in lock(&self.streams).iter() {
            stream.lost.store(true, Ordering::SeqCst);
        }
    }

    /// How many streams are open now: the sounds playing.
    pub fn playing(&self) -> usize {
        lock(&self.streams)
            .iter()
            .filter(|stream| stream.open.load(Ordering::SeqCst))
            .count()
    }

    /// How many streams were ever opened.
    pub fn opened(&self) -> usize {
        lock(&self.streams).len()
    }

    /// How many frames each stream was given, in the order they opened.
    pub fn frames(&self) -> Vec<u64> {
        lock(&self.streams)
            .iter()
            .map(|stream| stream.frames.load(Ordering::SeqCst))
            .collect()
    }

    /// The rate each stream was opened at, in the order they opened.
    pub fn rates(&self) -> Vec<u32> {
        lock(&self.streams)
            .iter()
            .map(|stream| stream.rate.load(Ordering::SeqCst) as u32)
            .collect()
    }

    /// The loudest sample each stream was given, from 0 to 1.
    pub fn peaks(&self) -> Vec<f32> {
        lock(&self.streams)
            .iter()
            .map(|stream| f32::from_bits(stream.peak.load(Ordering::SeqCst) as u32))
            .collect()
    }
}

impl Output for StandInSound {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        if self.no_device.load(Ordering::SeqCst) {
            return Err(OpenError::NoDevice(
                "there is no sound output device".into(),
            ));
        }
        let stream = Arc::new(Stream::default());
        stream.rate.store(u64::from(rate), Ordering::SeqCst);
        stream.open.store(true, Ordering::SeqCst);
        lock(&self.streams).push(stream.clone());
        let speed = match self.speed.load(Ordering::SeqCst) {
            0 => 4800,
            speed => speed,
        };
        Ok(Box::new(StandInSink { stream, speed }))
    }
}

struct StandInSink {
    stream: Arc<Stream>,
    speed: usize,
}

impl StandInSink {
    fn device(&self) -> Result<(), String> {
        if self.stream.lost.load(Ordering::SeqCst) {
            return Err("the sound output device was removed".into());
        }
        Ok(())
    }
}

impl Sink for StandInSink {
    fn room(&mut self) -> Result<usize, String> {
        self.device()?;
        Ok(self.speed)
    }

    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        self.device()?;
        self.stream
            .frames
            .fetch_add((frames.len() / 2) as u64, Ordering::SeqCst);
        let loudest = frames
            .iter()
            .fold(0f32, |peak, sample| peak.max(sample.abs()));
        self.stream
            .peak
            .fetch_max(u64::from(loudest.to_bits()), Ordering::SeqCst);
        Ok(())
    }

    fn idle(&mut self) -> Result<bool, String> {
        self.device()?;
        Ok(true)
    }
}

impl Drop for StandInSink {
    fn drop(&mut self) {
        self.stream.open.store(false, Ordering::SeqCst);
    }
}
