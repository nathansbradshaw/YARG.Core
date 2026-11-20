//! Regenerable, copyright-free five-player song for native/Unity acceptance.
use std::{fs, io::Write, path::PathBuf};
use yace_stepmania::{Document, Format};

fn variable(mut value: u32) -> Vec<u8> {
    let mut bytes = vec![(value & 127) as u8];
    while {
        value >>= 7;
        value > 0
    } {
        bytes.push(((value & 127) | 128) as u8);
    }
    bytes.reverse();
    bytes
}
fn track(name: &str, mut events: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    events.insert(
        0,
        (
            0,
            [vec![0xff, 3, name.len() as u8], name.as_bytes().to_vec()].concat(),
        ),
    );
    events.sort_by_key(|e| e.0);
    let (mut bytes, mut last) = (Vec::new(), 0);
    for (tick, event) in events {
        bytes.extend(variable(tick - last));
        bytes.extend(event);
        last = tick;
    }
    bytes.extend([0, 0xff, 0x2f, 0]);
    [
        b"MTrk".to_vec(),
        (bytes.len() as u32).to_be_bytes().to_vec(),
        bytes,
    ]
    .concat()
}
fn note(events: &mut Vec<(u32, Vec<u8>)>, tick: u32, length: u32, key: u8) {
    events.push((tick, vec![0x90, key, 100]));
    events.push((tick + length, vec![0x80, key, 0]));
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("Usage: fixture OUTPUT")?);
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("song.ini"),
        "[song]\nname = YACE Mixed Band Check\nartist = YARG Test Fixture\ncharter = YARG\nsong_length = 14000\ndiff_guitar = 1\ndiff_drums = 1\ndiff_vocals = 1\n",
    )?;
    let mut wave = fs::File::create(root.join("song.wav"))?;
    let count = 44100u32 * 14;
    wave.write_all(b"RIFF")?;
    wave.write_all(&(36 + count * 2).to_le_bytes())?;
    wave.write_all(b"WAVEfmt ")?;
    wave.write_all(&16u32.to_le_bytes())?;
    wave.write_all(&1u16.to_le_bytes())?;
    wave.write_all(&1u16.to_le_bytes())?;
    wave.write_all(&44100u32.to_le_bytes())?;
    wave.write_all(&88200u32.to_le_bytes())?;
    wave.write_all(&2u16.to_le_bytes())?;
    wave.write_all(&16u16.to_le_bytes())?;
    wave.write_all(b"data")?;
    wave.write_all(&(count * 2).to_le_bytes())?;
    // Deliberately silent: the fixture exercises the audio clock, not listening acceptance.
    wave.write_all(&vec![0; count as usize * 2])?;
    let mut guitar = Vec::new();
    let mut drums = Vec::new();
    let mut vocals = Vec::new();
    for beat in 4..24 {
        note(&mut guitar, beat * 480, 120, 96 + (beat % 4) as u8);
        note(&mut drums, beat * 480, 120, 97 + (beat % 3) as u8);
    }
    for phrase in [4, 12, 20] {
        note(&mut vocals, phrase * 480, 1440, 105);
        for beat in phrase..phrase + 3 {
            vocals.push((beat * 480, vec![0xff, 5, 2, b'L', b'a']));
            note(&mut vocals, beat * 480, 400, 60);
        }
    }
    let mut midi = b"MThd\0\0\0\x06\0\x01\0\x04\x01\xe0".to_vec();
    midi.extend(track(
        "TEMPO",
        vec![
            (0, vec![0xff, 0x51, 3, 7, 0xa1, 0x20]),
            (0, vec![0xff, 0x58, 4, 4, 2, 24, 8]),
        ],
    ));
    midi.extend(track("PART GUITAR", guitar));
    midi.extend(track("PART DRUMS", drums));
    midi.extend(track("PART VOCALS", vocals));
    fs::write(root.join("notes.mid"), midi)?;
    let text = "#VERSION:0.83;\n#TITLE:YACE Mixed Band Check;\n#ARTIST:YARG Test Fixture;\n#MUSIC:song.wav;\n#OFFSET:0;\n#BPMS:0=120;\n#STOPS:8=0.25;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DESCRIPTION:YACE pad fixture;\n#DIFFICULTY:Challenge;\n#METER:6;\n#NOTES:\n0000\n0000\n0000\n0000\n,\n1000\n0100\n0010\n0001\n,\n2000\n0100\n3000\n0001\n,\n1000\n0100\n0010\n0001\n,\n1000\n0100\n0010\n0001\n,\n1000\n0100\n0010\n0001\n;\n";
    for (folder, keyboard) in [("YACE-pad", false), ("YACE-keyboard", true)] {
        let dir = root.join(folder);
        fs::create_dir_all(&dir)?;
        let raw = if keyboard {
            text.replace("YACE pad fixture", "YACE keyboard fixture")
                .replace("1000\n0100\n0010\n0001", "1100\n0011\n1100\n0011")
        } else {
            text.into()
        };
        let document = Document::parse(raw, Format::Ssc)?;
        fs::write(dir.join("notes.ssc"), document.export(Format::Ssc)?)?;
        if !dir.join("song.wav").exists() {
            fs::hard_link(root.join("song.wav"), dir.join("song.wav"))?;
        }
    }
    println!("{}", root.display());
    Ok(())
}
