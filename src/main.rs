mod audio;
mod corpus;
mod duration;
mod features;
mod join;
mod lexicon;
mod postlex;
mod syllables;
mod synthesis;
mod target_cost;
mod text;
mod tone;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use audio::write_wav;
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
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: maxim-native lookup|pronounce LANG.DAT VOICE.DLL WORD [POS] | inspect VOX | diphone VOX NAME | phones|syllables|phones-simple VOX TEXT | features VOX UNIT_ID | duration-model VOX PHONE | duration-text VOX TEXT | decode-packet VOX OFFSET OUTPUT.wav | decode-unit VOX INDEX OUTPUT.wav | dump-diphone VOX NAME OUTPUT.mdu | speak VOX TEXT OUTPUT.wav [--basic-join|--acoustic-join] [--simple-phones] [--heuristic-target]");
        std::process::exit(2);
    }
    if args[1] == "lookup" || args[1] == "pronounce" {
        let plugin = args.get(3).ok_or("missing voice plugin path")?;
        let word = args.get(4).ok_or("missing word")?;
        let pos = args.get(5).and_then(|s| s.chars().next());
        let lexicon = Lexicon::open(Path::new(&args[2]), Path::new(plugin))?;
        if args[1] == "pronounce" {
            println!("{}", lexicon.pronounce(word, pos)?.join(" "));
        } else {
            match lexicon.lookup(word, pos) {
                Some(phones) => println!("{}", phones.join(" ")),
                None => println!("{word}: not in lexicon (LTS fallback required)"),
            }
        }
        eprintln!("loaded {} lexicon entries", lexicon.len());
        return Ok(());
    }
    let mut db = VoiceDb::open(Path::new(&args[2]))?;
    match args[1].as_str() {
        "inspect" => {
            println!(
                "name={} sample_rate={} codec={} checksum={:08x} content_start={:#x} file_size={}",
                db.name,
                db.sample_rate,
                db.codec,
                db.checksum,
                db.content_start(),
                db.size
            );
            for s in &db.sections {
                println!(
                    "section {:>2} offset={:#x} bytes={}",
                    s.kind, s.offset, s.len
                );
            }
        }
        "diphone" => {
            let name = args.get(3).ok_or("missing diphone name")?;
            match db.diphone(name)? {
                Some((start, count, values)) => println!(
                    "{name}: candidate_start={start} count={count} first_unit_ids={values:?}"
                ),
                None => println!("{name}: absent"),
            }
        }
        "phones" => {
            let input = args.get(3).ok_or("missing text")?;
            let lexicon = load_original_lexicon(Path::new(&args[2]))?;
            println!("{}", phonemes_original(input, &lexicon)?.join(" "));
        }
        "syllables" => {
            let input = args.get(3).ok_or("missing text")?;
            let lexicon = load_original_lexicon(Path::new(&args[2]))?;
            let pronounced = pronounced_original(input, &lexicon)?;
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
        "phones-simple" => {
            let input = args.get(3).ok_or("missing text")?;
            println!("{}", phonemes(input)?.join(" "));
        }
        "features" => {
            let id: u32 = args.get(3).ok_or("missing unit ID")?.parse()?;
            let feature_db = FeatureDb::load(&mut db)?;
            println!("{:#?}", feature_db.decode(&mut db, id)?);
        }
        "duration-model" => {
            let phone = args.get(3).ok_or("missing phone name")?;
            let duration = DurationDb::load(&mut db)?;
            let estimate = duration.estimate(phone, |_| None)?;
            println!(
                "phone={phone} base_ms={} scale_ms={} tree_value={} estimated_ms={}",
                estimate.base_ms, estimate.scale_ms, estimate.cart_value, estimate.duration_ms
            );
            println!(
                "unresolved target features: {}",
                estimate.missing_features.join(", ")
            );
        }
        "duration-text" => {
            let input = args.get(3).ok_or("missing text")?;
            let lexicon = load_original_lexicon(Path::new(&args[2]))?;
            let pronounced = pronounced_original(input, &lexicon)?;
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
        "decode-packet" => {
            if args.len() < 5 {
                return Err("missing offset or output path".into());
            }
            if db.codec != 14 {
                return Err("this decoder supports only codec 14".into());
            }
            let pos = u64::from_str_radix(args[3].trim_start_matches("0x"), 16)?;
            if pos < db.content_start() {
                return Err("packet offset precedes content section".into());
            }
            let rate = silk_rate(&db);
            let mut samples = Vec::new();
            let (mut packet, next) = db.packet(pos)?;
            let mut key = [0u8; 8];
            key[..4].copy_from_slice(&db.checksum.to_le_bytes());
            deobfuscate(&mut packet, &mut key);
            let mut decoder = SilkDecoder::new(rate as i32)?;
            decoder.decode_packet(&packet, &mut samples)?;
            write_wav(Path::new(&args[4]), &samples, db.sample_rate)?;
            eprintln!(
                "decoded {} samples from packet at {pos:#x}; next offset {next:#x}",
                samples.len()
            );
        }
        "decode-unit" => {
            if args.len() < 5 {
                return Err("missing unit index or output path".into());
            }
            let index: u32 = args[3].parse()?;
            let unit = decode_unit(&mut db, index)?;
            write_wav(Path::new(&args[4]), &unit.samples, db.sample_rate)?;
            eprintln!(
                "decoded unit {index}: {}/{} real samples",
                unit.available,
                unit.samples.len()
            );
        }
        "dump-diphone" => {
            if args.len() < 5 {
                return Err("missing diphone name or output path".into());
            }
            let ids = db.candidate_ids(&args[3], 10_000)?;
            let mut out = File::create(&args[4])?;
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
            eprintln!("dumped {decoded} decoded units for {} ({failed} failed, {empty} empty, {partial} partial)", args[3]);
        }
        "speak" => {
            if args.len() < 5 {
                return Err("missing text or output path".into());
            }
            let mut mode = synthesis::JoinMode::Original;
            let mut simple_phones = false;
            let mut structural_target = true;
            for option in args.iter().skip(5).map(String::as_str) {
                match option {
                    "--basic-join" if mode == synthesis::JoinMode::Original => {
                        mode = synthesis::JoinMode::Basic;
                    }
                    "--acoustic-join" if mode == synthesis::JoinMode::Original => {
                        mode = synthesis::JoinMode::Acoustic;
                    }
                    "--original-join" if mode == synthesis::JoinMode::Original => {}
                    "--simple-phones" if !simple_phones => simple_phones = true,
                    "--structural-target" => structural_target = true,
                    "--heuristic-target" => structural_target = false,
                    _ => return Err(format!("unknown or duplicate speak option {option}").into()),
                }
            }
            let pronounced = if simple_phones {
                let names = phonemes(&args[3])?;
                let count = names.len();
                text::Pronounced {
                    stress: vec![None; count],
                    names,
                    syllables: Vec::new(),
                    words: Vec::new(),
                    punctuation: vec![None; count],
                }
            } else {
                let lexicon = load_original_lexicon(Path::new(&args[2]))?;
                pronounced_original(&args[3], &lexicon)?
            };
            eprintln!("phones: {}", pronounced.names.join(" "));
            let targets = if structural_target && !simple_phones {
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
                Path::new(&args[4]),
                mode,
                targets.as_deref(),
            )?;
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
