//! The ringing sound on macOS: an AudioToolbox audio queue on the default
//! output device, so Core Audio mixes it with every other application's
//! sound at the output's own volume (Pane never changes it) and converts
//! the sound's rate to the device's. The queue is fed from the ringing's
//! own thread; Core Audio's own thread hands each buffer back once it was
//! played. The functions are the system's own frameworks', declared here:
//! no audio crate.
//!
//! No default output device (Core Audio's
//! `kAudioHardwarePropertyDefaultOutputDevice` is none) is no output
//! device: `not-available`. The system's sound for alerts is the one the
//! user chose in Sound settings (`com.apple.sound.beep.sound`), an AIFF
//! file Pane reads itself, else macOS's Tink.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use objc2_foundation::{NSString, NSUserDefaults};

use super::{OpenError, Output, Sink};

/// `OSStatus`: 0 for success.
type Status = i32;
/// `AudioQueueRef`.
type Queue = *mut c_void;

/// `AudioStreamBasicDescription`.
#[repr(C)]
#[allow(dead_code, reason = "read by Core Audio, not by Pane")]
struct StreamFormat {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

/// `AudioQueueBuffer`.
#[repr(C)]
#[allow(dead_code, reason = "read by Core Audio, not by Pane")]
struct QueueBuffer {
    capacity: u32,
    data: *mut c_void,
    size: u32,
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

/// `AudioObjectPropertyAddress`.
#[repr(C)]
#[allow(dead_code, reason = "read by Core Audio, not by Pane")]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

/// `AudioQueueOutputCallback`.
type Played = extern "C" fn(*mut c_void, Queue, *mut QueueBuffer);

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewOutput(
        format: *const StreamFormat,
        callback: Played,
        user_data: *mut c_void,
        run_loop: *const c_void,
        run_loop_mode: *const c_void,
        flags: u32,
        queue: *mut Queue,
    ) -> Status;
    fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, buffer: *mut *mut QueueBuffer) -> Status;
    fn AudioQueueEnqueueBuffer(
        queue: Queue,
        buffer: *mut QueueBuffer,
        packet_descriptions: u32,
        descriptions: *const c_void,
    ) -> Status;
    fn AudioQueueStart(queue: Queue, start_time: *const c_void) -> Status;
    fn AudioQueueStop(queue: Queue, immediate: u8) -> Status;
    fn AudioQueueDispose(queue: Queue, immediate: u8) -> Status;
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> Status;
}

/// `kAudioFormatLinearPCM` ('lpcm').
const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
/// `kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked`.
const FLOAT_PACKED: u32 = 1 | 8;
/// `kAudioObjectSystemObject`.
const SYSTEM_OBJECT: u32 = 1;
/// `kAudioHardwarePropertyDefaultOutputDevice` ('dOut').
const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
/// `kAudioObjectPropertyScopeGlobal` ('glob').
const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
/// `kAudioObjectPropertyElementMain`.
const ELEMENT_MAIN: u32 = 0;

/// The buffers a queue cycles through.
const BUFFERS: usize = 4;
/// The frames one buffer holds: four of them are 200 ms at 48 kHz, enough
/// that a busy moment never starves the device, short enough that a stop
/// is heard at once (the queue is stopped, not drained).
const BUFFER_FRAMES: usize = 2400;
/// Bytes in a stereo float frame.
const FRAME_BYTES: usize = 8;

/// The system's audio output through an audio queue.
pub(super) struct AudioQueue;

impl Output for AudioQueue {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        if !has_output_device() {
            return Err(OpenError::NoDevice(
                "there is no sound output device".into(),
            ));
        }
        let format = StreamFormat {
            sample_rate: f64::from(rate),
            format_id: LINEAR_PCM,
            format_flags: FLOAT_PACKED,
            bytes_per_packet: FRAME_BYTES as u32,
            frames_per_packet: 1,
            bytes_per_frame: FRAME_BYTES as u32,
            channels_per_frame: 2,
            bits_per_channel: 32,
            reserved: 0,
        };
        let free = Arc::new(Free::default());
        let mut queue: Queue = std::ptr::null_mut();
        // SAFETY: `format` outlives the call; the callback's user data is
        // the free list, which the stream keeps alive until the queue is
        // disposed of, after which the callback is never called. With no
        // run loop the callback runs on Core Audio's own thread.
        let made = unsafe {
            AudioQueueNewOutput(
                &format,
                played,
                Arc::as_ptr(&free).cast_mut().cast(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut queue,
            )
        };
        if made != 0 {
            return Err(OpenError::Failed(format!(
                "macOS did not open the sound output (error {made})"
            )));
        }
        // From here the stream disposes of the queue if anything fails.
        let mut stream = Stream {
            queue,
            free,
            started: false,
        };
        for _ in 0..BUFFERS {
            let mut buffer = std::ptr::null_mut();
            // SAFETY: the queue was made above; `buffer` is written by the
            // call and owned by the queue, which frees it when disposed of.
            let allocated = unsafe {
                AudioQueueAllocateBuffer(
                    stream.queue,
                    (BUFFER_FRAMES * FRAME_BYTES) as u32,
                    &mut buffer,
                )
            };
            if allocated != 0 {
                return Err(OpenError::Failed(format!(
                    "macOS did not give the sound output a buffer (error {allocated})"
                )));
            }
            stream.free.lock().push(Buffer(buffer));
        }
        // SAFETY: the queue was made above; started at once, so a device
        // that cannot play answers before `ring` does.
        let started = unsafe { AudioQueueStart(stream.queue, std::ptr::null()) };
        if started != 0 {
            return Err(OpenError::NoDevice(format!(
                "the sound output device does not play (error {started})"
            )));
        }
        stream.started = true;
        Ok(Box::new(stream))
    }
}

/// Whether Core Audio has a default output device.
fn has_output_device() -> bool {
    let address = PropertyAddress {
        selector: DEFAULT_OUTPUT,
        scope: SCOPE_GLOBAL,
        element: ELEMENT_MAIN,
    };
    let mut device = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: the system object's default output device, an `AudioObjectID`
    // (a `u32`) written into `device`, whose size `size` says.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut device as *mut u32).cast(),
        )
    };
    // `kAudioObjectUnknown` is 0.
    status == 0 && device != 0
}

/// A buffer of the queue's, while it is Pane's to fill.
struct Buffer(*mut QueueBuffer);

// SAFETY: a buffer is handed between the ringing's thread, which fills
// and enqueues it, and Core Audio's thread, which hands it back once
// played; only one of them has it at a time, through the free list.
unsafe impl Send for Buffer {}

/// The buffers the queue handed back, free to fill.
#[derive(Default)]
struct Free(Mutex<Vec<Buffer>>);

impl Free {
    fn lock(&self) -> MutexGuard<'_, Vec<Buffer>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The queue played `buffer`: it is free again.
extern "C" fn played(user_data: *mut c_void, _queue: Queue, buffer: *mut QueueBuffer) {
    // SAFETY: the user data is the stream's free list, alive until the
    // queue is disposed of (see `AudioQueue::open`).
    let free = unsafe { &*user_data.cast::<Free>() };
    free.lock().push(Buffer(buffer));
}

/// An open audio queue, fed by one ringing's thread.
struct Stream {
    queue: Queue,
    free: Arc<Free>,
    started: bool,
}

impl Sink for Stream {
    fn room(&mut self) -> Result<usize, String> {
        Ok(self.free.lock().len() * BUFFER_FRAMES)
    }

    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        for chunk in frames.chunks(BUFFER_FRAMES * 2) {
            let Some(Buffer(buffer)) = self.free.lock().pop() else {
                // More than the room was never asked for.
                break;
            };
            let bytes = size_of_val(chunk);
            // SAFETY: a free buffer of the queue's, of BUFFER_FRAMES frames'
            // capacity, which `chunk` fits; it is the queue's again once
            // enqueued.
            let enqueued = unsafe {
                std::ptr::copy_nonoverlapping(
                    chunk.as_ptr().cast::<u8>(),
                    (*buffer).data.cast::<u8>(),
                    bytes,
                );
                (*buffer).size = bytes as u32;
                AudioQueueEnqueueBuffer(self.queue, buffer, 0, std::ptr::null())
            };
            if enqueued != 0 {
                self.free.lock().push(Buffer(buffer));
                return Err(format!("the sound output device failed (error {enqueued})"));
            }
        }
        Ok(())
    }

    fn idle(&mut self) -> Result<bool, String> {
        Ok(self.free.lock().len() == BUFFERS)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the queue made in `open`, stopped at once (what it holds
        // is dropped, not played) and disposed of once, synchronously: no
        // callback runs afterwards, so the free list may go.
        unsafe {
            if self.started {
                AudioQueueStop(self.queue, 1);
            }
            AudioQueueDispose(self.queue, 1);
        }
    }
}

/// The sound the user chose for alerts in Sound settings, else macOS's
/// Tink, which every macOS has.
pub(super) fn alert_sound() -> PathBuf {
    let chosen = NSUserDefaults::standardUserDefaults()
        .stringForKey(&NSString::from_str("com.apple.sound.beep.sound"))
        .map(|path| PathBuf::from(path.to_string()));
    chosen.unwrap_or_else(|| PathBuf::from("/System/Library/Sounds/Tink.aiff"))
}

#[cfg(test)]
mod tests {
    //! The audio queue on the real output: opted in with
    //! `PANE_TEST_REAL_ALERTS`, which CI's macOS test jobs set
    //! (`.github/actions/setup`), so a developer's own Mac never sounds.
    //! What the runner has (an output device or none) is recorded in the
    //! test's output.

    use std::time::{Duration, Instant};

    use super::*;

    use crate::real_alerts::{opted_in, record};

    #[test]
    fn the_queue_plays_and_dropping_it_stops_it_or_there_is_no_device() {
        if !opted_in() {
            return;
        }
        let mut sink = match AudioQueue.open(48_000) {
            Ok(sink) => sink,
            Err(OpenError::NoDevice(why)) => {
                record(&format!("the audio queue: {why}"));
                return;
            }
            Err(OpenError::Failed(why)) => panic!("the audio queue failed: {why}"),
        };
        record("the audio queue opened on the default output device");
        assert_eq!(sink.room().unwrap(), BUFFERS * BUFFER_FRAMES);
        // A quiet half second, in two writes.
        let frames = vec![0.0f32; BUFFERS * BUFFER_FRAMES * 2];
        sink.write(&frames).unwrap();
        assert_eq!(sink.room().unwrap(), 0);
        // The device plays the buffers and hands them back.
        let began = Instant::now();
        while sink.room().unwrap() == 0 {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the device played nothing"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        sink.write(&frames).unwrap();
        // Dropped while it holds sound: it stops at once.
        let dropping = Instant::now();
        drop(sink);
        assert!(dropping.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_alert_sound_is_a_file() {
        if !opted_in() {
            return;
        }
        let sound = alert_sound();
        record(&format!("the alert sound is {}", sound.display()));
        assert!(sound.is_file(), "{}", sound.display());
    }
}
