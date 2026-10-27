//! The ringing engine with the stand-in output: each way a ringing ends
//! stops its sound and leaves its generation's undo list empty, and the
//! limits, the repeats, the volume and the refusals are as ADR 0053 says.
//! The guest's side, through real extensions, is `tests/ringing.rs`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::Ordering;

use super::sounds::{Source, Wav, aiff_file, wav_file};
use super::*;
use crate::generation::End;

const OWNER: &str = "local:/somewhere";

const PROMPTLY: Duration = Duration::from_secs(10);

/// A ringer playing through a fresh stand-in, with the logs it writes to.
fn ringer() -> (Arc<Ringer>, Arc<StandInSound>, ExtensionLogs) {
    let logs = ExtensionLogs::default();
    let ringer = Arc::new(Ringer::new(logs.clone()));
    let stand_in = StandInSound::new();
    ringer.set_output(stand_in.clone());
    (ringer, stand_in, logs)
}

/// What a test rings: `sound`, `repeat`, at full volume, held by instance
/// `instance` of the fixture, in `generation`.
fn asked(
    owner: &str,
    instance: u64,
    generation: &Generation,
    sound: Sound,
    repeat: Repeat,
) -> Ring {
    Ring {
        owner: owner.into(),
        component: "/somewhere/fixture.wasm".into(),
        instance: (1, instance),
        generation: Some(generation.clone()),
        sound,
        repeat,
        volume: 100,
    }
}

/// Rings the alarm until stopped for `owner`, answering its id.
fn ring_alarm(ringer: &Arc<Ringer>, owner: &str, instance: u64, generation: &Generation) -> u64 {
    let ring = asked(
        owner,
        instance,
        generation,
        Sound::Bundled(Bundled::Alarm),
        Repeat::UntilStopped,
    );
    block_on(ringer.ring(ring).unwrap().started()).unwrap()
}

/// Waits until `done` holds, saying `what` did not.
fn until(what: &str, done: impl Fn() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < PROMPTLY, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The sound plays: the stand-in has an open stream taking frames.
fn assert_playing(stand_in: &StandInSound) {
    until("the sound did not play", || {
        stand_in.playing() == 1 && stand_in.frames().iter().sum::<u64>() > 0
    });
}

/// The sound stopped: no stream is open, nothing more is written, nothing
/// is held, and the undo list is empty.
fn assert_stopped(ringer: &Ringer, stand_in: &StandInSound, generation: &Generation) {
    until("the sound did not stop", || stand_in.playing() == 0);
    let written = stand_in.frames();
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(stand_in.frames(), written, "the sound went on");
    // The ringing thread drops its output before it takes the ringing off
    // the registry, so a busy runner can still hold it here.
    until("the ringing was still held", || {
        ringer.count() == 0 && generation.undo_list().is_empty()
    });
}

/// A package's own WAV file of `frames` frames, every sample `level`.
fn wav(folder: &Path, frames: usize, level: i16) -> std::path::PathBuf {
    let path = folder.join("sound.wav");
    std::fs::write(&path, wav_file(8_000, &vec![[level, level]; frames])).unwrap();
    path
}

#[test]
fn dropping_the_handle_stops_the_sound_and_takes_it_off_the_undo_list() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    let id = ring_alarm(&ringer, OWNER, 1, &generation);
    assert_playing(&stand_in);
    assert_eq!(generation.undo_list(), ["ringing sound"]);

    ringer.release(id);

    assert_stopped(&ringer, &stand_in, &generation);
    // Dropping it again, as a handle of one that ended, does nothing.
    ringer.release(id);
}

#[test]
fn the_instance_going_stops_its_sounds_and_no_other() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    ring_alarm(&ringer, OWNER, 1, &generation);
    let other = ring_alarm(&ringer, "local:/other", 2, &generation);
    until("both did not play", || stand_in.playing() == 2);

    ringer.instance_gone(Path::new("/somewhere/fixture.wasm"), (1, 1));

    until("the instance's sound did not stop", || {
        stand_in.playing() == 1
    });
    assert_eq!(ringer.count(), 1);
    ringer.release(other);
    assert_stopped(&ringer, &stand_in, &generation);
}

#[test]
fn every_end_of_the_generation_stops_the_sound_and_empties_the_undo_list() {
    for why in [
        End::Disabled,
        End::Paused,
        End::Replaced,
        End::Uninstalled,
        End::Abandoned,
    ] {
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();
        ring_alarm(&ringer, OWNER, 1, &generation);
        assert_playing(&stand_in);

        drop(generation.end(why));

        assert_stopped(&ringer, &stand_in, &generation);
    }
}

#[test]
fn quitting_stops_every_sound_and_starts_none_afterwards() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    ring_alarm(&ringer, OWNER, 1, &generation);
    ring_alarm(&ringer, "local:/other", 2, &generation);
    until("both did not play", || stand_in.playing() == 2);

    ringer.stop_all();

    assert_stopped(&ringer, &stand_in, &generation);
    let again = ringer.ring(asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Bell),
        Repeat::Once,
    ));
    assert!(matches!(again, Err(RingError::NotAvailable(_))));
}

/// A guest whose call was stopped while its sound started (the answer's
/// future dropped) holds no handle: the sound stops too.
#[test]
fn a_start_given_up_stops_the_sound() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    let starting = ringer
        .ring(asked(
            OWNER,
            1,
            &generation,
            Sound::Bundled(Bundled::Alarm),
            Repeat::UntilStopped,
        ))
        .unwrap();

    drop(starting);

    until("it did not stop", || {
        ringer.count() == 0 && stand_in.playing() == 0
    });
    assert_eq!(generation.undo_list(), Vec::<&str>::new());
}

#[test]
fn a_generation_that_ended_rings_nothing() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    drop(generation.end(End::Replaced));

    let refused = ringer.ring(asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Chime),
        Repeat::Once,
    ));

    assert!(matches!(refused, Err(RingError::Failed(_))));
    assert_eq!(ringer.count(), 0);
    assert_eq!(stand_in.opened(), 0);
}

#[test]
fn two_sounds_per_package_and_eight_in_all() {
    let (ringer, stand_in, _) = ringer();
    let generation = Generation::new();
    ring_alarm(&ringer, OWNER, 1, &generation);
    ring_alarm(&ringer, OWNER, 1, &generation);
    let third = ringer.ring(asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Alarm),
        Repeat::UntilStopped,
    ));
    let Err(RingError::Limit(why)) = third else {
        panic!("a third sound of one package was not refused");
    };
    assert!(why.contains("already rings 2 sounds"), "{why}");
    for package in 1..=3 {
        let owner = format!("local:/other{package}");
        ring_alarm(&ringer, &owner, 2, &generation);
        ring_alarm(&ringer, &owner, 2, &generation);
    }
    assert_eq!(ringer.count(), 8);
    let ninth = ringer.ring(asked(
        "local:/another",
        3,
        &generation,
        Sound::Bundled(Bundled::Bell),
        Repeat::Once,
    ));
    let Err(RingError::Limit(why)) = ninth else {
        panic!("a ninth sound was not refused");
    };
    assert!(why.contains("already ring 8 sounds"), "{why}");
    until("all did not play", || stand_in.playing() == 8);
    drop(generation.end(End::Disabled));
    assert_stopped(&ringer, &stand_in, &generation);
}

#[test]
fn a_sound_played_as_often_as_asked_ends_by_itself() {
    let folder = tempfile::tempdir().unwrap();
    let path = wav(folder.path(), 1000, 8000);
    for (repeat, frames) in [
        (Repeat::Once, 1000),
        (Repeat::Times(3), 3000),
        (Repeat::Times(100), 100_000),
    ] {
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();
        block_on(
            ringer
                .ring(asked(
                    OWNER,
                    1,
                    &generation,
                    Sound::File(path.clone()),
                    repeat,
                ))
                .unwrap()
                .started(),
        )
        .unwrap();

        assert_stopped(&ringer, &stand_in, &generation);
        assert_eq!(stand_in.frames(), [frames], "{repeat:?}");
        assert_eq!(stand_in.rates(), [8_000]);
    }
}

#[test]
fn a_ringing_stops_by_itself_at_the_limit_and_says_so() {
    let (ringer, stand_in, logs) = ringer();
    ringer.set_limit(Duration::from_millis(200));
    let generation = Generation::new();
    let began = Instant::now();
    ring_alarm(&ringer, OWNER, 1, &generation);
    assert_playing(&stand_in);

    assert_stopped(&ringer, &stand_in, &generation);
    assert!(began.elapsed() >= Duration::from_millis(200));
    let lines = logs.lines(OWNER);
    assert!(
        lines
            .iter()
            .any(|line| line.text.contains("Pane's limit for one ringing")),
        "{lines:?}"
    );
}

#[test]
fn the_volume_scales_the_sound_and_over_100_is_100() {
    let folder = tempfile::tempdir().unwrap();
    let path = wav(folder.path(), 100, 16_384);
    for (volume, peak) in [(100, 0.5), (200, 0.5), (50, 0.25), (0, 0.0)] {
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();
        let mut ring = asked(
            OWNER,
            1,
            &generation,
            Sound::File(path.clone()),
            Repeat::Once,
        );
        ring.volume = volume;
        block_on(ringer.ring(ring).unwrap().started()).unwrap();
        assert_stopped(&ringer, &stand_in, &generation);
        assert_eq!(stand_in.peaks(), [peak], "volume {volume}");
    }
}

#[test]
fn every_bundled_sound_plays_at_its_level() {
    for bundled in [Bundled::Alarm, Bundled::Bell, Bundled::Chime] {
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();
        let ring = asked(OWNER, 1, &generation, Sound::Bundled(bundled), Repeat::Once);
        block_on(ringer.ring(ring).unwrap().started()).unwrap();
        assert_stopped(&ringer, &stand_in, &generation);
        assert_eq!(stand_in.rates(), [48_000]);
        let [frames] = stand_in.frames()[..] else {
            panic!("{bundled:?} did not play once");
        };
        // Short sounds: a second or two each.
        assert!(
            (24_000..=120_000).contains(&frames),
            "{bundled:?}: {frames}"
        );
        let [peak] = stand_in.peaks()[..] else {
            unreachable!()
        };
        assert!((peak - 0.7).abs() < 0.01, "{bundled:?}: {peak}");
    }
}

/// Where Pane has no adapter (a system other than Windows, macOS and
/// Linux), every ringing is not available, and nothing is held.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn without_an_adapter_every_ringing_is_not_available() {
    let ringer = Arc::new(Ringer::new(ExtensionLogs::default()));
    let generation = Generation::new();

    let answer = ringer.ring(asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Bell),
        Repeat::Once,
    ));

    assert!(matches!(answer, Err(RingError::NotAvailable(_))));
    assert_eq!(ringer.count(), 0);
    assert_eq!(generation.undo_list(), Vec::<&str>::new());
}

#[test]
fn no_output_device_is_not_available() {
    let (ringer, stand_in, _) = ringer();
    stand_in.set_device(false);
    let generation = Generation::new();
    let ring = asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Bell),
        Repeat::Once,
    );

    let answer = block_on(ringer.ring(ring).unwrap().started());

    assert_eq!(
        answer,
        Err(RingError::NotAvailable(
            "there is no sound output device".into()
        ))
    );
    until("it was still held", || ringer.count() == 0);
    assert_eq!(generation.undo_list(), Vec::<&str>::new());
}

#[test]
fn a_file_that_cannot_be_played_fails_saying_why() {
    let folder = tempfile::tempdir().unwrap();
    let missing = folder.path().join("missing.wav");
    let broken = folder.path().join("broken.wav");
    std::fs::write(&broken, b"RIFF\x04\0\0\0WAVE").unwrap();
    let garbage = folder.path().join("garbage.mp3");
    std::fs::write(&garbage, b"this is no sound at all").unwrap();
    let large = folder.path().join("large.wav");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    for (path, says) in [
        (&missing, "cannot be read"),
        (&broken, "has no format chunk"),
        (&garbage, "garbage.mp3"),
        (&large, "at most 10 MiB"),
    ] {
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();
        let ring = asked(
            OWNER,
            1,
            &generation,
            Sound::File(path.clone()),
            Repeat::Once,
        );

        let answer = block_on(ringer.ring(ring).unwrap().started());

        let Err(RingError::Failed(why)) = answer else {
            panic!("{} was not refused: {answer:?}", path.display());
        };
        assert!(why.contains(says), "{why}");
        until("it was still held", || ringer.count() == 0);
        assert_eq!(stand_in.opened(), 0);
    }
}

#[test]
fn a_device_lost_mid_ring_ends_it_and_writes_to_the_extension_log() {
    let (ringer, stand_in, logs) = ringer();
    let generation = Generation::new();
    ring_alarm(&ringer, OWNER, 1, &generation);
    assert_playing(&stand_in);

    stand_in.lose_device();

    assert_stopped(&ringer, &stand_in, &generation);
    let lines = logs.lines(OWNER);
    assert!(
        lines
            .iter()
            .any(|line| line.text
                == "The ringing sound stopped: the sound output device was removed"),
        "{lines:?}"
    );
}

#[test]
fn wav_files_of_each_encoding_are_read() {
    /// A WAV file of `channels` channels of `bits`-bit samples encoded as
    /// `tag` (1 integer, 3 float), extensible or not, holding `samples`.
    fn file(tag: u16, bits: u16, channels: u16, extensible: bool, samples: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut format = Vec::new();
        format.extend_from_slice(&(if extensible { 0xFFFE } else { tag }).to_le_bytes());
        format.extend_from_slice(&channels.to_le_bytes());
        format.extend_from_slice(&8_000u32.to_le_bytes());
        format.extend_from_slice(&(8_000 * u32::from(block)).to_le_bytes());
        format.extend_from_slice(&block.to_le_bytes());
        format.extend_from_slice(&bits.to_le_bytes());
        if extensible {
            format.extend_from_slice(&22u16.to_le_bytes());
            format.extend_from_slice(&bits.to_le_bytes());
            format.extend_from_slice(&3u32.to_le_bytes());
            format.extend_from_slice(&tag.to_le_bytes());
            format.extend_from_slice(&[0; 14]);
        }
        let mut file = b"RIFF\0\0\0\0WAVE".to_vec();
        file.extend_from_slice(b"fmt ");
        file.extend_from_slice(&(format.len() as u32).to_le_bytes());
        file.extend_from_slice(&format);
        file.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        file.extend_from_slice(samples);
        file
    }
    let half = [
        (file(1, 8, 1, false, &[192]), "8-bit"),
        (file(1, 16, 1, false, &16_384i16.to_le_bytes()), "16-bit"),
        (file(1, 24, 1, false, &[0, 0, 0x40]), "24-bit"),
        (
            file(1, 32, 1, false, &0x4000_0000i32.to_le_bytes()),
            "32-bit",
        ),
        (file(3, 32, 1, false, &0.5f32.to_le_bytes()), "float"),
        (file(3, 64, 1, false, &0.5f64.to_le_bytes()), "double"),
        (file(1, 16, 1, true, &16_384i16.to_le_bytes()), "extensible"),
    ];
    for (bytes, what) in half {
        let mut wav = Wav::read(bytes).unwrap().unwrap();
        let mut out = [0f32; 4];
        assert_eq!(wav.fill(&mut out), Ok(1), "{what}");
        assert_eq!(
            out[..2],
            [0.5, 0.5],
            "{what}: a mono sound plays on both sides"
        );
        assert_eq!(wav.fill(&mut out), Ok(0), "{what}");
        wav.rewind().unwrap();
        assert_eq!(wav.fill(&mut out), Ok(1), "{what}");
    }
    // Of more channels, the first two play.
    let mut samples = Vec::new();
    for sample in [8_192i16, -8_192, 32_767] {
        samples.extend_from_slice(&sample.to_le_bytes());
    }
    let mut wav = Wav::read(file(1, 16, 3, false, &samples)).unwrap().unwrap();
    let mut out = [0f32; 2];
    assert_eq!(wav.fill(&mut out), Ok(1));
    assert_eq!(out, [0.25, -0.25]);
    // An encoding Pane does not read itself is the system decoder's.
    assert!(Wav::read(file(2, 4, 1, false, &[0; 4])).unwrap().is_none());
    assert!(Wav::read(b"ID3 an mp3".to_vec()).unwrap().is_none());
}

/// macOS's system sounds are AIFF files: big-endian samples, the rate an
/// 80-bit float, the samples past the sound chunk's offset; AIFF-C's
/// uncompressed kinds too, and its compressed ones are refused.
#[test]
fn aiff_files_are_read() {
    let frames = [[16_384i16, -16_384], [0, 32_767]];
    let mut aiff = Wav::read_aiff(&aiff_file(44_100, &frames))
        .unwrap()
        .unwrap();
    assert_eq!(aiff.rate(), 44_100);
    let mut out = [0f32; 6];
    assert_eq!(aiff.fill(&mut out), Ok(2));
    assert_eq!(out[..4], [0.5, -0.5, 0.0, 32_767.0 / 32_768.0]);
    assert_eq!(aiff.fill(&mut out), Ok(0));
    aiff.rewind().unwrap();
    assert_eq!(aiff.fill(&mut out), Ok(2));

    /// `aiff_file`'s file as AIFF-C of `kind`, its samples replaced by
    /// `samples`, mono.
    fn aifc(kind: &[u8; 4], bits: u16, samples: &[u8]) -> Vec<u8> {
        let plain = aiff_file(8_000, &[[0, 0]]);
        let mut common = plain[20..38].to_vec();
        common[0..2].copy_from_slice(&1u16.to_be_bytes());
        common[6..8].copy_from_slice(&bits.to_be_bytes());
        common.extend_from_slice(kind);
        common.extend_from_slice(b"\0\0");
        let mut file = b"FORM\0\0\0\0AIFC".to_vec();
        file.extend_from_slice(b"COMM");
        file.extend_from_slice(&(common.len() as u32).to_be_bytes());
        file.extend_from_slice(&common);
        file.extend_from_slice(b"SSND");
        file.extend_from_slice(&(12 + samples.len() as u32).to_be_bytes());
        // An offset of 4: four bytes before the first sample.
        file.extend_from_slice(&4u32.to_be_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&[0xEE; 4]);
        file.extend_from_slice(samples);
        file
    }
    for (bytes, what) in [
        (aifc(b"NONE", 16, &16_384i16.to_be_bytes()), "big-endian"),
        (aifc(b"sowt", 16, &16_384i16.to_le_bytes()), "little-endian"),
        (aifc(b"fl32", 32, &0.5f32.to_be_bytes()), "float"),
        (aifc(b"fl64", 64, &0.5f64.to_be_bytes()), "double"),
        (aifc(b"NONE", 8, &[64]), "8-bit"),
        (aifc(b"NONE", 24, &[0x40, 0, 0]), "24-bit"),
    ] {
        let mut aiff = Wav::read_aiff(&bytes)
            .unwrap_or_else(|why| panic!("{what}: {why}"))
            .unwrap();
        assert_eq!(aiff.rate(), 8_000, "{what}");
        let mut out = [0f32; 2];
        assert_eq!(aiff.fill(&mut out), Ok(1), "{what}");
        assert_eq!(out, [0.5, 0.5], "{what}: a mono sound plays on both sides");
    }
    let compressed = Wav::read_aiff(&aifc(b"ulaw", 8, &[0])).err().unwrap();
    assert!(compressed.contains("compressed (ulaw)"), "{compressed}");
    // Not an AIFF file at all: none.
    assert!(Wav::read_aiff(&wav_file(8_000, &frames)).unwrap().is_none());
}

/// Linux's system sound is the freedesktop sound theme's, Ogg Vorbis or
/// WAV, the most fitting name first; none is not available, saying so.
#[test]
fn the_sound_theme_s_notification_sound_is_found() {
    let home = tempfile::tempdir().unwrap();
    let system = tempfile::tempdir().unwrap();
    let folders = [home.path().to_path_buf(), system.path().to_path_buf()];
    let stereo = system.path().join("freedesktop/stereo");

    let none = super::sounds::theme_sound(&folders).err().unwrap();
    assert!(none.contains("not installed"), "{none}");

    std::fs::create_dir_all(&stereo).unwrap();
    std::fs::write(stereo.join("bell.wav"), b"RIFF").unwrap();
    assert_eq!(
        super::sounds::theme_sound(&folders),
        Ok(stereo.join("bell.wav"))
    );
    std::fs::write(stereo.join("message-new-instant.oga"), b"OggS").unwrap();
    assert_eq!(
        super::sounds::theme_sound(&folders),
        Ok(stereo.join("message-new-instant.oga"))
    );
    // The user's own theme folder comes first.
    let mine = home.path().join("freedesktop/stereo");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("message-new-instant.wav"), b"RIFF").unwrap();
    assert_eq!(
        super::sounds::theme_sound(&folders),
        Ok(mine.join("message-new-instant.wav"))
    );
}

/// Half a second of an 880 Hz tone at 22,050 Hz, mono, at an eighth of full
/// scale, made for these tests with ffmpeg (`fixtures/README.md`).
const TONE_MP3: &[u8] = include_bytes!("fixtures/tone.mp3");
const TONE_OGG: &[u8] = include_bytes!("fixtures/tone.ogg");

/// An MP3 and an Ogg Vorbis file play on every system: Symphonia decodes
/// them (Media Foundation an MP3 on Windows), at the file's rate, as often
/// as asked, and on both sides.
#[test]
fn mp3_and_ogg_vorbis_files_play() {
    let folder = tempfile::tempdir().unwrap();
    for (name, bytes) in [("tone.mp3", TONE_MP3), ("tone.ogg", TONE_OGG)] {
        let path = folder.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let (ringer, stand_in, _) = ringer();
        let generation = Generation::new();

        let answer = block_on(
            ringer
                .ring(asked(
                    OWNER,
                    1,
                    &generation,
                    Sound::File(path.clone()),
                    Repeat::Times(2),
                ))
                .unwrap()
                .started(),
        );

        assert!(answer.is_ok(), "{name}: {answer:?}");
        assert_stopped(&ringer, &stand_in, &generation);
        assert_eq!(stand_in.rates(), [22_050], "{name}");
        let [frames] = stand_in.frames()[..] else {
            unreachable!()
        };
        // Twice half a second, give or take the encoder's padding.
        assert!((20_000..=28_000).contains(&frames), "{name}: {frames}");
        let [peak] = stand_in.peaks()[..] else {
            unreachable!()
        };
        assert!(peak > 0.1, "{name} played silence: {peak}");
    }
}

/// Symphonia's own reading: a mono file plays on both sides, rewinding
/// plays it again from the start, and a file that only looks like one is
/// refused saying so.
#[test]
fn compressed_files_are_decoded_and_rewound() {
    for (name, bytes) in [("tone.mp3", TONE_MP3), ("tone.ogg", TONE_OGG)] {
        let mut sound =
            super::compressed::Compressed::open(Path::new(name), bytes.to_vec()).unwrap();
        assert_eq!(sound.rate(), 22_050, "{name}");
        let mut out = vec![0f32; 4_096];
        let mut first = 0;
        let mut loudest = 0f32;
        loop {
            let frames = sound.fill(&mut out).unwrap();
            if frames == 0 {
                break;
            }
            for [left, right] in out[..frames * 2].as_chunks::<2>().0 {
                assert_eq!(left, right, "{name}: a mono sound plays on both sides");
                loudest = loudest.max(left.abs());
            }
            first += frames;
        }
        assert!(loudest > 0.1, "{name}: {loudest}");
        sound.rewind().unwrap();
        let mut again = 0;
        loop {
            let frames = sound.fill(&mut out).unwrap();
            if frames == 0 {
                break;
            }
            again += frames;
        }
        assert_eq!(first, again, "{name}");
    }
    let fake = super::compressed::Compressed::open(
        Path::new("fake.ogg"),
        b"OggS but nothing after".to_vec(),
    );
    let Err(why) = fake else {
        panic!("a broken Ogg file was decoded");
    };
    assert!(why.contains("fake.ogg"), "{why}");
}

/// The system's own output, with what it was given and whether a stream of
/// it is open counted: what the real-output test watches.
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct Watched {
    output: Arc<dyn Output>,
    open: Arc<std::sync::atomic::AtomicUsize>,
    frames: Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct WatchedSink {
    sink: Box<dyn Sink>,
    open: Arc<std::sync::atomic::AtomicUsize>,
    frames: Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Output for Watched {
    fn unavailable(&self) -> Option<String> {
        self.output.unavailable()
    }

    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        let sink = self.output.open(rate)?;
        self.open.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(WatchedSink {
            sink,
            open: self.open.clone(),
            frames: self.frames.clone(),
        }))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Sink for WatchedSink {
    fn room(&mut self) -> Result<usize, String> {
        self.sink.room()
    }

    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        self.sink.write(frames)?;
        self.frames
            .fetch_add((frames.len() / 2) as u64, Ordering::SeqCst);
        Ok(())
    }

    fn idle(&mut self) -> Result<bool, String> {
        self.sink.idle()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for WatchedSink {
    fn drop(&mut self) {
        self.open.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The real output plays a ringing, and dropping its handle stops it: on
/// Linux into ALSA's `null` device (a CI runner has no sound card), on
/// macOS into the default output device, when the runner has one (what it
/// has is recorded in the test's output). Opted in with
/// `PANE_TEST_REAL_ALERTS`, which CI's test jobs set on Linux and macOS
/// (`.github/actions/setup`), so a developer's own Mac never sounds.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn the_system_output_plays_a_ringing_and_dropping_it_stops_it() {
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    if !crate::real_alerts::opted_in() {
        return;
    }
    #[cfg(target_os = "linux")]
    let output: Arc<dyn Output> = Arc::new(super::alsa::AlsaOutput::named("null"));
    #[cfg(target_os = "macos")]
    let output: Arc<dyn Output> = Arc::new(super::macos::AudioQueue);
    let open = Arc::new(AtomicUsize::new(0));
    let frames = Arc::new(AtomicU64::new(0));
    let ringer = Arc::new(Ringer::new(ExtensionLogs::default()));
    ringer.set_output(Arc::new(Watched {
        output,
        open: open.clone(),
        frames: frames.clone(),
    }));
    let generation = Generation::new();
    let ring = asked(
        OWNER,
        1,
        &generation,
        Sound::Bundled(Bundled::Alarm),
        Repeat::UntilStopped,
    );

    let id = match block_on(ringer.ring(ring).unwrap().started()) {
        Ok(id) => id,
        Err(RingError::NotAvailable(why)) if cfg!(target_os = "macos") => {
            crate::real_alerts::record(&format!("the system's output is not available: {why}"));
            return;
        }
        Err(other) => panic!("the system's output did not play: {other:?}"),
    };
    until("the sound did not play", || {
        frames.load(Ordering::SeqCst) >= 24_000
    });
    assert_eq!(open.load(Ordering::SeqCst), 1);
    crate::real_alerts::record(&format!(
        "the system's output took {} frames of a ringing, then stopped",
        frames.load(Ordering::SeqCst)
    ));

    ringer.release(id);

    until("the sound did not stop", || {
        open.load(Ordering::SeqCst) == 0
    });
    let written = frames.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(frames.load(Ordering::SeqCst), written, "the sound went on");
    assert_eq!(ringer.count(), 0);
    assert_eq!(generation.undo_list(), Vec::<&str>::new());
}
