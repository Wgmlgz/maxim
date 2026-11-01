//! Cached results of the deterministic SILK validity and coverage checks.
//!
//! Selection only needs the decoded sample count, not the sample values. The
//! winning units are still decoded normally before they are rendered.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use sha2::{Digest, Sha256};

use crate::corpus::{decode_unit, VoiceDb};

const MAGIC: &[u8; 8] = b"MAXUIDX1";
const HEADER_LEN: usize = 8 + 8 + 4 + 4 + 4 + 4 + 32;

pub(crate) struct UnitIndex {
    records: Vec<u8>,
}

impl UnitIndex {
    pub(crate) fn path_for(vox: &Path) -> PathBuf {
        let mut name = vox.as_os_str().to_owned();
        name.push(".unitidx");
        name.into()
    }

    fn count(db: &VoiceDb) -> Result<usize, Box<dyn Error>> {
        let section = db.section(1)?;
        if section.len % 8 != 0 {
            return Err("invalid unit table length".into());
        }
        Ok((section.len / 8) as usize)
    }

    /// Return None only when the optional index is absent or belongs to a
    /// different voice. A damaged matching index is an error.
    pub(crate) fn load(db: &VoiceDb) -> Result<Option<Self>, Box<dyn Error>> {
        let path = Self::path_for(&db.path);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if data.len() < HEADER_LEN || &data[..8] != MAGIC {
            return Err(format!("invalid unit index: {}", path.display()).into());
        }
        let u64_at = |offset| u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        let u32_at = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let count = Self::count(db)?;
        if u64_at(8) != db.size
            || u32_at(16) != db.checksum
            || u32_at(20) != db.sample_rate
            || u32_at(24) != db.codec
            || u32_at(28) as usize != count
        {
            return Ok(None);
        }
        let records = &data[HEADER_LEN..];
        if records.len() != count * 4 || Sha256::digest(records).as_slice() != &data[32..64] {
            return Err(format!("corrupt unit index: {}", path.display()).into());
        }
        Ok(Some(Self {
            records: records.to_vec(),
        }))
    }

    /// Returns (wanted samples, available samples). None means the original
    /// decoder rejected the unit or produced no usable samples.
    pub(crate) fn coverage(&self, id: u32) -> Option<(usize, usize)> {
        let row = self.records.get(id as usize * 4..id as usize * 4 + 4)?;
        let wanted = u16::from_le_bytes(row[..2].try_into().unwrap()) as usize;
        let available = u16::from_le_bytes(row[2..].try_into().unwrap()) as usize;
        (wanted > 0 && available > 0 && available <= wanted).then_some((wanted, available))
    }

    pub(crate) fn build(vox: &Path, jobs: usize) -> Result<PathBuf, Box<dyn Error>> {
        let db = VoiceDb::open(vox)?;
        let count = Self::count(&db)?;
        let jobs = jobs.max(1).min(count);
        let chunk = count.div_ceil(jobs);
        let chunks: Result<Vec<_>, Box<dyn Error>> = thread::scope(|scope| {
            let handles: Vec<_> = (0..jobs)
                .map(|worker| {
                    let start = worker * chunk;
                    let end = ((worker + 1) * chunk).min(count);
                    scope.spawn(move || -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
                        let mut db = VoiceDb::open(vox).map_err(|e| e.to_string())?;
                        let mut bytes = Vec::with_capacity((end - start) * 4);
                        for id in start..end {
                            let (wanted, available) = decode_unit(&mut db, id as u32)
                                .ok()
                                .map(|unit| (unit.samples.len(), unit.available))
                                .filter(|(wanted, available)| {
                                    *wanted > 0 && *available > 0 && *wanted <= u16::MAX as usize
                                })
                                .unwrap_or((0, 0));
                            bytes.extend_from_slice(&(wanted as u16).to_le_bytes());
                            bytes.extend_from_slice(&(available as u16).to_le_bytes());
                        }
                        Ok(bytes)
                    })
                })
                .collect();
            let mut chunks = Vec::with_capacity(handles.len());
            for handle in handles {
                chunks.push(
                    handle
                        .join()
                        .map_err(|_| "unit index worker panicked")?
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok(chunks)
        });
        let mut records = Vec::with_capacity(count * 4);
        for part in chunks? {
            records.extend_from_slice(&part);
        }
        if records.len() != count * 4 {
            return Err("unit index generation produced wrong record count".into());
        }
        let mut data = Vec::with_capacity(HEADER_LEN + records.len());
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&db.size.to_le_bytes());
        data.extend_from_slice(&db.checksum.to_le_bytes());
        data.extend_from_slice(&db.sample_rate.to_le_bytes());
        data.extend_from_slice(&db.codec.to_le_bytes());
        data.extend_from_slice(&(count as u32).to_le_bytes());
        data.extend_from_slice(&Sha256::digest(&records));
        data.extend_from_slice(&records);
        let path = Self::path_for(vox);
        let temporary = path.with_extension("unitidx.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_coverage_matches_decoder() {
        let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
        let mut db = VoiceDb::open(path).unwrap();
        let index = UnitIndex::load(&db).unwrap().expect("bundled unit index");
        let count = UnitIndex::count(&db).unwrap();
        for iteration in 0..128 {
            let id = ((iteration * 7919 + 305) % count) as u32;
            let decoded = decode_unit(&mut db, id)
                .ok()
                .filter(|unit| !unit.samples.is_empty() && unit.available > 0)
                .map(|unit| (unit.samples.len(), unit.available));
            assert_eq!(index.coverage(id), decoded, "unit {id}");
        }
    }
}
