use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub(crate) fn write_wav(path: &Path, samples: &[i16], rate: u32) -> Result<(), Box<dyn Error>> {
    let mut f = File::create(path)?;
    let data_len = u32::try_from(samples.len() * 2)?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

pub(crate) fn finish_utterance(samples: &mut Vec<i16>, rate: u32) {
    let fade = samples.len().min((rate / 100) as usize);
    for i in 0..fade {
        let pos = samples.len() - fade + i;
        samples[pos] = (samples[pos] as i32 * (fade - i) as i32 / fade as i32) as i16;
    }
}

pub(crate) fn normalize_leading_silence(samples: &mut Vec<i16>, rate: u32) {
    let frame = (rate / 100) as usize;
    let target = (rate * 9 / 100) as usize;
    let onset = samples
        .chunks(frame)
        .position(|chunk| {
            let energy: i64 = chunk.iter().map(|&x| i64::from(x) * i64::from(x)).sum();
            energy > 250 * 250 * chunk.len() as i64
        })
        .map(|i| i * frame);
    let Some(onset) = onset else {
        return;
    };
    if onset > target {
        samples.drain(..onset - target);
    } else if onset < target {
        samples.splice(..0, std::iter::repeat_n(0, target - onset));
    }
}
