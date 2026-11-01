use std::error::Error;
use std::io::IsTerminal;
use std::path::Path;
use std::thread;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::audio::{finish_utterance, normalize_leading_silence, write_wav};
use crate::corpus::{decode_unit, DecodedUnit, VoiceDb};
use crate::features::FeatureDb;
use crate::join::{self, JoinClass};
use crate::target_cost::{self, FeatureRecord};
use crate::unit_index::UnitIndex;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum JoinMode {
    Basic,
    Acoustic,
    Original,
}

struct SynthesisProgress {
    bar: ProgressBar,
    pair_count: u64,
}

impl SynthesisProgress {
    fn new(pair_count: usize, verbose: bool) -> Self {
        let bar = if std::io::stderr().is_terminal() && !verbose {
            let bar = ProgressBar::new(1000);
            bar.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} {msg} [{wide_bar:.cyan/blue}] {percent}%",
                )
                .expect("valid progress style")
                .progress_chars("=>-"),
            );
            bar
        } else {
            ProgressBar::hidden()
        };
        bar.set_message("Selecting units");
        Self {
            bar,
            pair_count: pair_count as u64,
        }
    }

    fn stage(&self, message: &'static str) {
        self.bar.set_message(message);
    }

    fn selected(&self, count: usize) {
        // Candidate search and decoding dominate synthesis time. Reserve the
        // final 5% for path selection, stitching, and the WAV write.
        self.bar.set_position(count as u64 * 950 / self.pair_count);
    }

    fn rendered(&self, count: usize) {
        self.bar
            .set_position(950 + count as u64 * 40 / self.pair_count);
    }

    fn complete(&self) {
        self.bar.set_position(1000);
    }
}

impl Drop for SynthesisProgress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

// Original renderer FUN_18002f780 interpolates these 16-step weights when it
// overlaps the final pitch period of one unit with the first of the next.
const BLEND_WEIGHT: [f32; 17] = [
    1.0,
    0.9903926,
    0.96193975,
    0.9157348,
    0.8535534,
    0.7777851,
    0.6913417,
    0.59754515,
    0.5,
    0.40245485,
    0.30865827,
    0.22221488,
    0.14644662,
    0.084265195,
    0.038060233,
    0.00960736,
    0.0,
];

struct Candidate {
    id: u32,
    cost: i32,
    unit: Option<DecodedUnit>,
    coverage: Option<(usize, usize)>,
    start: Option<BoundaryFeatures>,
    end: Option<BoundaryFeatures>,
    row: Option<[u8; 36]>,
    next_row: Option<[u8; 36]>,
    join_class: JoinClass,
}

#[derive(Clone, Copy)]
struct BoundaryFeatures {
    period: usize,
    rms: f32,
    spectrum: [f32; 6],
}

fn boundary_features(samples: &[i16], rate: u32) -> BoundaryFeatures {
    let n = samples.len().max(1);
    let power: f32 = samples.iter().map(|&s| (s as f32) * (s as f32)).sum();
    let rms = (power / n as f32).sqrt();
    let mut spectrum = [0.0; 6];
    for (band, freq) in [350.0, 700.0, 1200.0, 2000.0, 3200.0, 4800.0]
        .into_iter()
        .enumerate()
    {
        let mut real = 0.0;
        let mut imag = 0.0;
        for (i, &sample) in samples.iter().enumerate() {
            let phase = std::f32::consts::TAU * freq * i as f32 / rate as f32;
            let window = if n > 1 {
                0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos()
            } else {
                1.0
            };
            real += sample as f32 * window * phase.cos();
            imag += sample as f32 * window * phase.sin();
        }
        spectrum[band] = ((real * real + imag * imag).sqrt() / (rms * n as f32 + 1.0) + 0.001).ln();
    }
    BoundaryFeatures {
        period: n,
        rms,
        spectrum,
    }
}

fn unit_boundaries(unit: &DecodedUnit, rate: u32) -> (BoundaryFeatures, BoundaryFeatures) {
    let first = unit.pitchmarks.first().copied().unwrap_or(0) as usize;
    let previous = unit
        .pitchmarks
        .get(unit.pitchmarks.len().saturating_sub(2))
        .copied()
        .unwrap_or(0) as usize;
    (
        boundary_features(&unit.samples[..first], rate),
        boundary_features(&unit.samples[previous..], rate),
    )
}

// Pause diphones in the corpus include the silence recorded around a source
// utterance. Keeping both sides of an internal pause can add several hundred
// milliseconds of silence between words.
pub(crate) fn trim_pause_silence(
    unit: &DecodedUnit,
    leading: bool,
    trailing: bool,
    rate: u32,
) -> DecodedUnit {
    if !leading && !trailing {
        return DecodedUnit {
            samples: unit.samples.clone(),
            pitchmarks: unit.pitchmarks.clone(),
            available: unit.available,
        };
    }
    let frame = (rate / 100).max(1) as usize;
    let rms: Vec<f64> = unit
        .samples
        .chunks(frame)
        .map(|chunk| {
            let power: f64 = chunk.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (power / chunk.len() as f64).sqrt()
        })
        .collect();
    let first = rms.iter().position(|&v| v > 25.0).unwrap_or(0) * frame;
    let last = rms
        .iter()
        .rposition(|&v| v > 25.0)
        .map(|i| ((i + 1) * frame).min(unit.samples.len()))
        .unwrap_or(unit.samples.len());
    let margin = (rate / 25) as usize; // about 40 ms of recorded silence
    let start_limit = if leading {
        first.saturating_sub(margin)
    } else {
        0
    };
    let end_limit = if trailing {
        (last + margin).min(unit.samples.len())
    } else {
        unit.samples.len()
    };
    let start = unit
        .pitchmarks
        .iter()
        .copied()
        .map(usize::from)
        .filter(|&mark| mark <= start_limit)
        .max()
        .unwrap_or(0);
    let end = unit
        .pitchmarks
        .iter()
        .copied()
        .map(usize::from)
        .find(|&mark| mark >= end_limit)
        .unwrap_or(unit.samples.len());
    if start >= end {
        return DecodedUnit {
            samples: unit.samples.clone(),
            pitchmarks: unit.pitchmarks.clone(),
            available: unit.available,
        };
    }
    DecodedUnit {
        samples: unit.samples[start..end].to_vec(),
        pitchmarks: unit
            .pitchmarks
            .iter()
            .copied()
            .map(usize::from)
            .filter(|&mark| mark > start && mark <= end)
            .map(|mark| (mark - start) as u16)
            .collect(),
        available: unit.available.saturating_sub(start).min(end - start),
    }
}

#[derive(Default)]
pub(crate) struct Stitcher {
    pub(crate) output: Vec<i16>,
    pending: Vec<i16>,
}

impl Stitcher {
    pub(crate) fn append(&mut self, unit: &DecodedUnit) -> Result<(), Box<dyn Error>> {
        let marks = &unit.pitchmarks;
        if marks.is_empty() || marks.last().copied().unwrap() as usize != unit.samples.len() {
            return Err("invalid unit pitchmarks".into());
        }
        let mut start = 0usize;
        for (i, &mark) in marks.iter().enumerate() {
            let end = mark as usize;
            if end <= start || end > unit.samples.len() {
                return Err("nonmonotonic unit pitchmarks".into());
            }
            let interval = &unit.samples[start..end];
            if i + 1 == marks.len() {
                self.pending = interval.to_vec();
            } else if i == 0 && !self.pending.is_empty() {
                let n = self.pending.len().min(interval.len());
                let current_tail = &interval[interval.len() - n..];
                for j in 0..n {
                    let phase = (j as f32 * 16.0) / n as f32;
                    let step = phase as usize;
                    let fraction = phase - step as f32;
                    let keep = BLEND_WEIGHT[step]
                        + fraction * (BLEND_WEIGHT[step + 1] - BLEND_WEIGHT[step]);
                    let mixed =
                        self.pending[j] as f32 * keep + current_tail[j] as f32 * (1.0 - keep);
                    self.output
                        .push(mixed.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
                }
                self.pending.clear();
            } else {
                self.output.extend_from_slice(interval);
            }
            start = end;
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) {
        self.output.append(&mut self.pending);
    }

    pub(crate) fn silence(&mut self, samples: usize) {
        self.flush();
        self.output.extend(std::iter::repeat_n(0, samples));
    }
}

fn candidates_for_pair(
    db: &mut VoiceDb,
    feature_db: &FeatureDb,
    mode: JoinMode,
    unit_index: Option<&UnitIndex>,
    phones: &[String],
    stress: &[Option<u8>],
    codes: &[u8],
    position: usize,
    targets: Option<&[FeatureRecord]>,
) -> Result<Vec<Candidate>, Box<dyn Error>> {
    let name = format!("{}-{}", phones[position], phones[position + 1]);
    let join_class = JoinClass::for_phones(&phones[position], &phones[position + 1]);
    if name == "pau-pau" {
        return Ok(vec![Candidate {
            id: u32::MAX,
            cost: 0,
            unit: None,
            coverage: None,
            start: None,
            end: None,
            row: None,
            next_row: None,
            join_class,
        }]);
    }
    let ids = db.candidate_ids(&name, 10_000)?;
    if ids.is_empty() {
        return Ok(vec![Candidate {
            id: u32::MAX,
            cost: 500,
            unit: None,
            coverage: None,
            start: None,
            end: None,
            row: None,
            next_row: None,
            join_class,
        }]);
    }
    let previous = if position == 0 {
        0
    } else {
        codes[position - 1]
    };
    let following = codes.get(position + 2).copied().unwrap_or(0);
    let mut ranked = Vec::with_capacity(ids.len());
    for id in ids {
        let meta = db.unit_metadata(id)?;
        let context_cost =
            50 * i32::from(meta[3] != previous) + 50 * i32::from(meta[6] != following);
        let mut stress_mismatch = 0;
        for (phone_index, candidate_id) in [(position, id), (position + 1, id + 1)] {
            if let Some(expected) = stress[phone_index] {
                if feature_db.stress(db, candidate_id)? != expected {
                    stress_mismatch += 1;
                }
            }
        }
        let target_cost = if let Some(targets) = targets {
            let left = feature_db.decode(db, id)?;
            let right = feature_db.decode(db, id.saturating_add(1))?;
            let left = FeatureRecord::from_candidate(&left);
            let right = FeatureRecord::from_candidate(&right);
            let left_cost = if phones[position] == "pau" {
                target_cost::known_single(&left, &targets[position])
            } else {
                0
            };
            let right_cost = if phones[position + 1] == "pau" {
                target_cost::known_single(&right, &targets[position + 1])
            } else {
                0
            };
            target_cost::known_pair(&left, &right, &targets[position], &targets[position + 1])
                - left_cost
                - right_cost
        } else {
            0
        };
        let score = if targets.is_some() {
            target_cost
        } else {
            context_cost + 1000 * stress_mismatch
        };
        ranked.push((stress_mismatch, score, context_cost, id, meta));
    }
    if targets.is_some() {
        // FUN_180031120 admits matching-stress candidates first. Its second
        // pass runs only if fewer than 51 matched; FUN_180030d90 then sorts
        // the admitted set by target cost.
        let matching = ranked.iter().filter(|item| item.0 == 0).count();
        if matching >= 51 {
            ranked.retain(|item| item.0 == 0);
        }
        ranked.sort_unstable_by_key(|&(_, score, _, id, _)| (score, id));
    } else {
        ranked.sort_unstable();
    }
    let cutoff = if targets.is_some() && ranked.len() > 51 {
        ranked[50].1
    } else {
        i32::MAX
    };
    let mut complete = Vec::new();
    let mut partial = Vec::new();
    let scan_limit = if targets.is_some() { usize::MAX } else { 2000 };
    for (_stress_mismatch, score, context_cost, id, meta) in ranked.into_iter().take(scan_limit) {
        if targets.is_some() && score > cutoff && context_cost == 100 {
            continue;
        }
        let (unit, coverage, start, end) = if let Some(index) = unit_index {
            let Some(coverage) = index.coverage(id) else {
                continue;
            };
            (None, Some(coverage), None, None)
        } else {
            let Ok(unit) = decode_unit(db, id) else {
                continue;
            };
            if unit.available == 0 {
                continue;
            }
            let coverage = (unit.samples.len(), unit.available);
            let (start, end) = if mode == JoinMode::Original {
                (None, None)
            } else {
                let (start, end) = unit_boundaries(&unit, db.sample_rate);
                (Some(start), Some(end))
            };
            (Some(unit), Some(coverage), start, end)
        };
        let (wanted, available) = coverage.unwrap();
        let missing = wanted - available;
        let coverage_cost = (missing * 200 / wanted) as i32;
        let candidate = Candidate {
            id,
            cost: score + coverage_cost,
            unit,
            coverage,
            start,
            end,
            row: Some(meta),
            next_row: db.unit_metadata(id.saturating_add(1)).ok(),
            join_class,
        };
        if missing == 0 {
            complete.push(candidate);
            if targets.is_none() && complete.len() >= 64 {
                break;
            }
        } else {
            partial.push(candidate);
        }
    }
    let mut candidates = if complete.is_empty() {
        partial.sort_by_key(|c| c.cost);
        if targets.is_none() {
            partial.truncate(64);
        }
        partial
    } else {
        complete
    };
    if candidates.is_empty() {
        candidates.push(Candidate {
            id: u32::MAX,
            cost: 500,
            unit: None,
            coverage: None,
            start: None,
            end: None,
            row: None,
            next_row: None,
            join_class,
        });
    }
    if candidates.is_empty() {
        return Err(format!("no decodable candidate for {name}").into());
    }
    Ok(candidates)
}

fn join_cost(left: &Candidate, right: &Candidate, mode: JoinMode, budget: i32) -> i32 {
    if mode == JoinMode::Original {
        if left.id.checked_add(1) == Some(right.id) {
            return 0;
        }
        return match (&left.next_row, &right.row) {
            (Some(a), Some(b)) => join::cost(left.join_class, a, b, budget),
            _ => 0,
        };
    }
    let (Some(a), Some(b)) = (left.end, right.start) else {
        return 0;
    };
    if left.id.checked_add(1) == Some(right.id) {
        return -80;
    }
    // The original selector uses a much richer linguistic target model. Until
    // that model is ported, compare decoded boundary periods to avoid choosing
    // recordings with an obvious energy or spectral jump at a splice.
    let period = ((a.period as i32 - b.period as i32).abs() / 5).min(30);
    if mode == JoinMode::Basic {
        return 20 + period;
    }
    let energy = ((a.rms.max(1.0) / b.rms.max(1.0)).log2().abs() * 18.0) as i32;
    let spectral = if a.rms > 100.0 && b.rms > 100.0 {
        (a.spectrum
            .iter()
            .zip(&b.spectrum)
            .map(|(x, y)| (x - y).abs())
            .sum::<f32>()
            * 5.0) as i32
    } else {
        0
    };
    20 + period + energy.min(45) + spectral.min(45)
}

fn best_path(layers: &[Vec<Candidate>], mode: JoinMode) -> Vec<usize> {
    let mut costs: Vec<i32> = layers[0].iter().map(|c| c.cost).collect();
    let mut back = Vec::new();
    for layer in 1..layers.len() {
        let mut next = vec![i32::MAX; layers[layer].len()];
        let mut pointers = vec![0usize; layers[layer].len()];
        let mut beam: Vec<usize> = (0..costs.len()).collect();
        beam.sort_unstable_by_key(|&index| (costs[index], index));
        if mode == JoinMode::Original {
            beam.truncate(50);
        }
        for (j, candidate) in layers[layer].iter().enumerate() {
            for &i in &beam {
                let previous = &layers[layer - 1][i];
                let base = costs[i] + candidate.cost;
                let transition = join_cost(previous, candidate, mode, next[j].saturating_sub(base));
                let cost = base + transition;
                if cost < next[j] {
                    next[j] = cost;
                    pointers[j] = i;
                }
            }
        }
        costs = next;
        back.push(pointers);
    }
    let mut index = costs
        .iter()
        .enumerate()
        .min_by_key(|(_, cost)| *cost)
        .unwrap()
        .0;
    let mut path = vec![index];
    for pointers in back.iter().rev() {
        index = pointers[index];
        path.push(index);
    }
    path.reverse();
    path
}

pub(crate) fn speak(
    db: &mut VoiceDb,
    phones: &[String],
    stress: &[Option<u8>],
    output_path: &Path,
    mode: JoinMode,
    targets: Option<&[FeatureRecord]>,
    verbose: bool,
    jobs: usize,
) -> Result<(), Box<dyn Error>> {
    if phones.len() != stress.len() {
        return Err("phone and stress counts disagree".into());
    }
    if targets.is_some_and(|records| records.len() != phones.len()) {
        return Err("phone and target record counts disagree".into());
    }
    if phones.len() < 2 {
        let silence = vec![0; (db.sample_rate / 5) as usize];
        write_wav(output_path, &silence, db.sample_rate)?;
        eprintln!("Wrote {}", output_path.display());
        return Ok(());
    }
    let progress = SynthesisProgress::new(phones.len() - 1, verbose);
    let profile = std::env::var_os("MAXIM_PROFILE").is_some();
    let started = Instant::now();
    let unit_index = if mode == JoinMode::Original {
        UnitIndex::load(db)?
    } else {
        None
    };
    let feature_db = FeatureDb::load(db)?;
    let codes: Vec<u8> = phones
        .iter()
        .map(|p| db.phone_code(p))
        .collect::<Result<_, _>>()?;
    let prepared = Instant::now();
    let pair_count = phones.len() - 1;
    let workers = if jobs == 0 {
        thread::available_parallelism()
            .map_or(1, usize::from)
            .min(8)
    } else {
        jobs.min(16)
    }
    .min(pair_count);
    let mut layers = Vec::with_capacity(pair_count);
    for position in 0..pair_count {
        layers.push(candidates_for_pair(
            db,
            &feature_db,
            mode,
            unit_index.as_ref(),
            phones,
            stress,
            &codes,
            position,
            targets,
        )?);
        progress.selected(position + 1);
    }
    let selected = Instant::now();
    progress.stage("Choosing path");
    let path = best_path(&layers, mode);
    let searched = Instant::now();
    progress.stage("Rendering");
    let mut decoded_units: Vec<Option<DecodedUnit>> =
        std::iter::repeat_with(|| None).take(path.len()).collect();
    if unit_index.is_some() && workers > 1 && path.len() >= 8 {
        progress.stage("Decoding selected units");
        let template = &*db;
        let results: Result<Vec<_>, Box<dyn Error>> = thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let path = &path;
                    let layers = &layers;
                    scope.spawn(move || -> Result<Vec<(usize, DecodedUnit)>, String> {
                        let mut local_db =
                            template.worker_copy().map_err(|error| error.to_string())?;
                        let mut result = Vec::new();
                        for position in (worker..path.len()).step_by(workers) {
                            let candidate = &layers[position][path[position]];
                            if candidate.id != u32::MAX && candidate.unit.is_none() {
                                let unit = decode_unit(&mut local_db, candidate.id)
                                    .map_err(|error| error.to_string())?;
                                result.push((position, unit));
                            }
                        }
                        Ok(result)
                    })
                })
                .collect();
            let mut results = Vec::with_capacity(workers);
            for handle in handles {
                results.push(handle.join().map_err(|_| "decode worker panicked")??);
            }
            Ok(results)
        });
        for worker in results? {
            for (position, unit) in worker {
                decoded_units[position] = Some(unit);
            }
        }
        progress.stage("Rendering");
    }
    let mut stitcher = Stitcher::default();
    for (position, &choice) in path.iter().enumerate() {
        let name = format!("{}-{}", phones[position], phones[position + 1]);
        let candidate = &layers[position][choice];
        if candidate.id != u32::MAX {
            let decoded;
            let unit =
                if let Some(unit) = candidate.unit.as_ref().or(decoded_units[position].as_ref()) {
                    unit
                } else {
                    decoded = decode_unit(db, candidate.id)?;
                    &decoded
                };
            if candidate.coverage != Some((unit.samples.len(), unit.available)) {
                return Err(
                    format!("unit index coverage disagrees for unit {}", candidate.id).into(),
                );
            }
            if verbose {
                eprintln!(
                    "{name}: unit {}, selection cost {}, {}/{} real samples",
                    candidate.id,
                    candidate.cost,
                    unit.available,
                    unit.samples.len()
                );
            }
            let trimmed = trim_pause_silence(
                unit,
                phones[position] == "pau",
                phones[position + 1] == "pau",
                db.sample_rate,
            );
            stitcher.append(&trimmed)?;
        } else {
            stitcher.silence((db.sample_rate / 12) as usize);
            if verbose {
                eprintln!("{name}: silence fallback");
            }
        }
        progress.rendered(position + 1);
    }
    progress.stage("Writing WAV");
    stitcher.flush();
    let rate = db.sample_rate;
    // The original renderer labels decoded SILK units with the VOX header
    // rate (FUN_18002c590) and concatenates their pitchmark periods directly
    // (FUN_18002cb40). The SILK decoder's internal 24 kHz rate is not a
    // second output-rate conversion for this 22.05 kHz voice.
    let mut output = stitcher.output;
    normalize_leading_silence(&mut output, rate);
    finish_utterance(&mut output, rate);
    write_wav(output_path, &output, rate)?;
    if profile {
        eprintln!(
            "timings: prepare={:?} selection={:?} path={:?} render/write={:?} total={:?}",
            prepared - started,
            selected - prepared,
            searched - selected,
            Instant::now() - searched,
            started.elapsed(),
        );
    }
    progress.complete();
    drop(progress);
    if verbose {
        eprintln!(
            "wrote {} samples at {rate} Hz to {}",
            output.len(),
            output_path.display()
        );
    } else {
        eprintln!("Wrote {}", output_path.display());
    }
    Ok(())
}
