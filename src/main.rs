mod audio;
mod cli;
mod corpus;
mod duration;
mod features;
mod join;
mod lexicon;
mod normalize;
mod playback;
mod postlex;
mod syllables;
mod synthesis;
mod target_cost;
mod text;
mod tone;
mod voice_file;

use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use audio::write_wav;
use cli::{Cli, Command, JoinChoice};
use corpus::{decode_unit, deobfuscate, silk_rate, SilkDecoder, VoiceDb};
use duration::DurationDb;
use features::FeatureDb;
use lexicon::Lexicon;
use text::{phonemes, phonemes_original, pronounced_original};

fn language_paths(vox: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let voice_dir = vox
        .parent()
        .ok_or("voice database has no parent directory")?;
    let app_dir = voice_dir
        .parent()
        .ok_or("voice directory has no app parent")?;
    let language = voice_dir.join("lang_ru_ru.dat");
    let plugin = app_dir.join("x64/voice_ru_maxim.dll");
    if !language.is_file() || !plugin.is_file() {
        return Err(
            "original Russian language data or voice DLL is missing beside the VOX tree".into(),
        );
    }
    Ok((language, plugin))
}

fn load_original_lexicon(vox: &Path) -> Result<Lexicon, Box<dyn Error>> {
    let (language, plugin) = language_paths(vox)?;
    Lexicon::open(&language, &plugin)
}

fn main() -> Result<(), Box<dyn Error>> {
    run(cli::parse())
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let pronounce = matches!(&cli.command, Command::Pronounce { .. });
    let vox = cli.voice;
    match cli.command {
        Command::Lookup { word, pos } | Command::Pronounce { word, pos } => {
            let lexicon = load_original_lexicon(&vox)?;
            if pronounce {
                println!("{}", lexicon.pronounce(&word, pos)?.join(" "));
            } else {
                match lexicon.lookup(&word, pos) {
                    Some(phones) => println!("{}", phones.join(" ")),
                    None => println!("{word}: not in lexicon (LTS fallback required)"),
                }
            }
            eprintln!("loaded {} lexicon entries", lexicon.len());
        }
        Command::Speak {
            text,
            output,
            join,
            simple_phones,
            heuristic_target,
            verbose,
            play,
        } => {
            let mut db = VoiceDb::open(&vox)?;
            let input = if text == "-" {
                let mut value = String::new();
                std::io::stdin().read_to_string(&mut value)?;
                value
            } else {
                text
            };
            let mode = match join {
                JoinChoice::Original => synthesis::JoinMode::Original,
                JoinChoice::Basic => synthesis::JoinMode::Basic,
                JoinChoice::Acoustic => synthesis::JoinMode::Acoustic,
            };
            let pronounced = if simple_phones {
                let names = phonemes(&input)?;
                let count = names.len();
                text::Pronounced {
                    stress: vec![None; count],
                    names,
                    syllables: Vec::new(),
                    words: Vec::new(),
                    punctuation: vec![None; count],
                }
            } else {
                let lexicon = load_original_lexicon(&vox)?;
                pronounced_original(&input, &lexicon)?
            };
            if verbose {
                eprintln!("phones: {}", pronounced.names.join(" "));
            }
            let targets = if !heuristic_target && !simple_phones {
                let duration = DurationDb::load(&mut db)?;
                let mut records = target_cost::text_records(&pronounced);
                for (position, record) in records.iter_mut().enumerate() {
                    if pronounced.names[position] != "pau" {
                        let estimate = duration.estimate_text(&pronounced, position)?;
                        record.set_duration(estimate.duration_ms.max(1) as f32 / 1000.0);
                    }
                }
                Some(records)
            } else {
                None
            };
            synthesis::speak(
                &mut db,
                &pronounced.names,
                &pronounced.stress,
                &output,
                mode,
                targets.as_deref(),
                verbose,
            )?;
            if play {
                playback::play(&output)?;
            }
        }
        Command::Inspect => {
            let db = VoiceDb::open(&vox)?;
            println!(
                "name={} sample_rate={} codec={} checksum={:08x} content_start={:#x} file_size={}",
                db.name,
                db.sample_rate,
                db.codec,
                db.checksum,
                db.content_start(),
                db.size
            );
            for section in &db.sections {
                println!(
                    "section {:>2} offset={:#x} bytes={}",
                    section.kind, section.offset, section.len
                );
            }
        }
        Command::Diphone { name } => {
            let mut db = VoiceDb::open(&vox)?;
            match db.diphone(&name)? {
                Some((start, count, values)) => println!(
                    "{name}: candidate_start={start} count={count} first_unit_ids={values:?}"
                ),
                None => println!("{name}: absent"),
            }
        }
        Command::Phones { text } => {
            let lexicon = load_original_lexicon(&vox)?;
            println!("{}", phonemes_original(&text, &lexicon)?.join(" "));
        }
        Command::Syllables { text } => {
            let lexicon = load_original_lexicon(&vox)?;
            let pronounced = pronounced_original(&text, &lexicon)?;
            for syllable in &pronounced.syllables {
                println!(
                    "word={}({}) syllable={} stress={} phones={}",
                    syllable.word + 1,
                    pronounced.words[syllable.word].name,
                    syllable.index_in_word + 1,
                    syllable.stress,
                    pronounced.names[syllable.start..syllable.end].join(" ")
                );
            }
        }
        Command::PhonesSimple { text } => println!("{}", phonemes(&text)?.join(" ")),
        Command::Features { unit_id } => {
            let mut db = VoiceDb::open(&vox)?;
            let feature_db = FeatureDb::load(&mut db)?;
            println!("{:#?}", feature_db.decode(&mut db, unit_id)?);
        }
        Command::DurationModel { phone } => {
            let mut db = VoiceDb::open(&vox)?;
            let duration = DurationDb::load(&mut db)?;
            let estimate = duration.estimate(&phone, |_| None)?;
            println!(
                "phone={phone} base_ms={} scale_ms={} tree_value={} estimated_ms={}",
                estimate.base_ms, estimate.scale_ms, estimate.cart_value, estimate.duration_ms
            );
            println!(
                "unresolved target features: {}",
                estimate.missing_features.join(", ")
            );
        }
        Command::DurationText { text } => {
            let mut db = VoiceDb::open(&vox)?;
            let lexicon = load_original_lexicon(&vox)?;
            let pronounced = pronounced_original(&text, &lexicon)?;
            let duration = DurationDb::load(&mut db)?;
            for (position, name) in pronounced.names.iter().enumerate() {
                if name == "pau" {
                    continue;
                }
                let estimate = duration.estimate_text(&pronounced, position)?;
                println!(
                    "{position:>3} {name:<4} {:>4} ms  unresolved={}",
                    estimate.duration_ms,
                    estimate.missing_features.join(",")
                );
            }
        }
        Command::DecodePacket { offset, output } => {
            let mut db = VoiceDb::open(&vox)?;
            if db.codec != 14 {
                return Err("this decoder supports only codec 14".into());
            }
            let position = u64::from_str_radix(offset.trim_start_matches("0x"), 16)?;
            if position < db.content_start() {
                return Err("packet offset precedes content section".into());
            }
            let rate = silk_rate(&db);
            let mut samples = Vec::new();
            let (mut packet, next) = db.packet(position)?;
            let mut key = [0u8; 8];
            key[..4].copy_from_slice(&db.checksum.to_le_bytes());
            deobfuscate(&mut packet, &mut key);
            let mut decoder = SilkDecoder::new(rate as i32)?;
            decoder.decode_packet(&packet, &mut samples)?;
            write_wav(&output, &samples, db.sample_rate)?;
            eprintln!(
                "decoded {} samples from packet at {position:#x}; next offset {next:#x}",
                samples.len()
            );
        }
        Command::DecodeUnit { index, output } => {
            let mut db = VoiceDb::open(&vox)?;
            let unit = decode_unit(&mut db, index)?;
            write_wav(&output, &unit.samples, db.sample_rate)?;
            eprintln!(
                "decoded unit {index}: {}/{} real samples",
                unit.available,
                unit.samples.len()
            );
        }
        Command::DumpDiphone { name, output } => {
            let mut db = VoiceDb::open(&vox)?;
            let ids = db.candidate_ids(&name, 10_000)?;
            let mut out = File::create(&output)?;
            out.write_all(b"MDU1")?;
            let mut decoded = 0;
            let mut failed = 0;
            let mut empty = 0;
            let mut partial = 0;
            for id in ids {
                let Ok(unit) = decode_unit(&mut db, id) else {
                    failed += 1;
                    continue;
                };
                if unit.available == 0 {
                    empty += 1;
                    continue;
                }
                if unit.available < unit.samples.len() {
                    partial += 1;
                }
                out.write_all(&id.to_le_bytes())?;
                out.write_all(&(unit.available as u32).to_le_bytes())?;
                for sample in &unit.samples[..unit.available] {
                    out.write_all(&sample.to_le_bytes())?;
                }
                decoded += 1;
            }
            eprintln!("dumped {decoded} decoded units for {name} ({failed} failed, {empty} empty, {partial} partial)");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
