//! Decodes the compact candidate feature layout used by the original selector.
//! The field offsets follow `tts_engine.dll` FUN_18002bdc0.  Unidentified
//! fields retain their original byte offsets instead of guessed meanings.

use std::error::Error;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::corpus::VoiceDb;

const NAME_WIDTH: usize = 32;

pub(crate) struct FeatureDb {
    section6: Vec<u8>,
    names: Vec<Arc<str>>,
    content_index: u8,
    has_extra: bool,
}

#[derive(Debug)]
#[allow(dead_code)] // The selector will consume these fields as it is ported.
pub(crate) struct CandidateFeatures {
    pub(crate) unit_id: u32,
    pub(crate) raw_row: [u8; 36],
    pub(crate) duration_seconds: f32,
    pub(crate) context_names: [Arc<str>; 5],
    pub(crate) section6_offset: u32,
    pub(crate) head_bytes: [u8; 22],
    pub(crate) middle_names: [Arc<str>; 4],
    pub(crate) tail_name: Arc<str>,
    pub(crate) middle_flags: [u8; 7],
    pub(crate) tail_flags: [u8; 8],
    pub(crate) content_flags: [bool; 3],
    pub(crate) optional_float: Option<f32>,
    pub(crate) optional_byte: Option<u8>,
}

fn u16le(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn Error>> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("feature record exceeds section 6")?
            .try_into()?,
    ))
}

fn u32le(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("feature record exceeds section 6")?
            .try_into()?,
    ))
}

impl FeatureDb {
    pub(crate) fn load(db: &mut VoiceDb) -> Result<Self, Box<dyn Error>> {
        let section6 = db.section(6)?;
        let section7 = db.section(7)?;
        if section7.len % NAME_WIDTH as u32 != 0 {
            return Err("invalid section-7 name table length".into());
        }
        db.file.seek(SeekFrom::Start(section6.offset as u64))?;
        let mut data = vec![0u8; section6.len as usize];
        db.file.read_exact(&mut data)?;
        db.file.seek(SeekFrom::Start(section7.offset as u64))?;
        let mut names_data = vec![0u8; section7.len as usize];
        db.file.read_exact(&mut names_data)?;
        let names: Vec<Arc<str>> = names_data
            .chunks_exact(NAME_WIDTH)
            .map(|slot| {
                let end = slot.iter().position(|&b| b == 0).unwrap_or(slot.len());
                Arc::from(String::from_utf8_lossy(&slot[..end]).as_ref())
            })
            .collect();
        let content_index = u8::try_from(
            names
                .iter()
                .position(|name| name.as_ref() == "content")
                .ok_or("missing content name in section 7")?,
        )?;
        let has_extra = if let Ok(section12) = db.section(12) {
            if section12.len == 0 {
                false
            } else {
                db.file.seek(SeekFrom::Start(section12.offset as u64))?;
                let mut flag = [0u8; 1];
                db.file.read_exact(&mut flag)?;
                flag[0] != 0
            }
        } else {
            false
        };
        Ok(Self {
            section6: data,
            names,
            content_index,
            has_extra,
        })
    }

    fn name(&self, index: u8) -> Result<Arc<str>, Box<dyn Error>> {
        Ok(self
            .names
            .get(index as usize)
            .ok_or("feature name index outside section 7")?
            .clone())
    }

    pub(crate) fn stress(&self, db: &mut VoiceDb, unit_id: u32) -> Result<u8, Box<dyn Error>> {
        let row = db.unit_metadata(unit_id)?;
        let offset = u32le(&row, 32)? as usize;
        Ok(self
            .section6
            .get(offset + 2)
            .ok_or("candidate stress exceeds section 6")?
            & 0x0f)
    }

    pub(crate) fn decode(
        &self,
        db: &mut VoiceDb,
        unit_id: u32,
    ) -> Result<CandidateFeatures, Box<dyn Error>> {
        let row = db.unit_metadata(unit_id)?;
        let offset = u32le(&row, 32)?;
        let head = self
            .section6
            .get(offset as usize..offset as usize + 22)
            .ok_or("candidate feature head exceeds section 6")?;
        let head_bytes: [u8; 22] = head.try_into()?;
        let first = (offset as usize)
            .checked_add(u16le(head, 15)? as usize)
            .ok_or("feature offset overflow")?;
        let middle = self
            .section6
            .get(first..first + 10)
            .ok_or("candidate middle features exceed section 6")?;
        let second = first
            .checked_add(u16le(middle, 8)? as usize)
            .ok_or("feature offset overflow")?;
        let tail = self
            .section6
            .get(second..second + 9)
            .ok_or("candidate tail features exceed section 6")?;
        let names = [row[2], row[3], row[4], row[5], row[6]]
            .map(|index| self.name(index))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| "invalid context-name count")?;
        let middle_names = [head[12], middle[0], middle[2], middle[6]]
            .map(|index| self.name(index))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| "invalid middle-name count")?;
        let tail_name = self.name(tail[6])?;
        Ok(CandidateFeatures {
            unit_id,
            raw_row: row,
            duration_seconds: u16le(&row, 0)? as f32 / 1000.0,
            context_names: names,
            section6_offset: offset,
            head_bytes,
            middle_names,
            tail_name,
            middle_flags: [
                middle[1],
                middle[3],
                middle[4],
                middle[5],
                middle[7],
                head[13] & 15,
                head[13] >> 4,
            ],
            tail_flags: [
                tail[0], tail[1], tail[2], tail[3], tail[4], tail[5], tail[7], tail[8],
            ],
            content_flags: [
                middle[0] == self.content_index,
                middle[2] == self.content_index,
                middle[6] == self.content_index,
            ],
            optional_float: self
                .has_extra
                .then(|| f32::from_le_bytes(head[17..21].try_into().unwrap())),
            optional_byte: self.has_extra.then_some(head[21]),
        })
    }
}
