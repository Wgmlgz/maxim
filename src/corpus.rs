use std::error::Error;
use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HEADER_LEN: usize = 104;
const MAX_SECTIONS: u32 = 13;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Section {
    pub(crate) kind: u32,
    pub(crate) offset: u32,
    pub(crate) len: u32,
}

#[derive(Debug)]
pub(crate) struct VoiceDb {
    pub(crate) file: File,
    pub(crate) size: u64,
    pub(crate) name: String,
    pub(crate) sample_rate: u32,
    pub(crate) codec: u32,
    pub(crate) checksum: u32,
    pub(crate) sections: Vec<Section>,
    metadata: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct UnitRecord {
    pub(crate) pitchmarks: Vec<u16>,
    pub(crate) audio_offset: u64,
    pub(crate) sample_skip: usize,
    pub(crate) encoded_bytes: usize,
}

pub(crate) struct DecodedUnit {
    pub(crate) samples: Vec<i16>,
    pub(crate) pitchmarks: Vec<u16>,
    pub(crate) available: usize,
}

fn u32le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

impl VoiceDb {
    pub(crate) fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut header = [0u8; HEADER_LEN];
        file.read_exact(&mut header)?;
        if &header[..4] != b"DDXV" || u32le(&header, 0x30) != 1 {
            return Err("unsupported DDXV voice header".into());
        }
        let count = u32le(&header, 0x34);
        if count > MAX_SECTIONS {
            return Err("too many voice sections".into());
        }
        let mut sections = Vec::new();
        for _ in 0..count {
            let mut entry = [0u8; 12];
            file.read_exact(&mut entry)?;
            let section = Section {
                kind: u32le(&entry, 0),
                offset: u32le(&entry, 4),
                len: u32le(&entry, 8),
            };
            let end = u64::from(section.offset) + u64::from(section.len);
            if end > size {
                return Err(format!("section {} exceeds file", section.kind).into());
            }
            sections.push(section);
        }
        let name_end = header[4..36].iter().position(|&x| x == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&header[4..4 + name_end]).to_string();
        Ok(Self {
            file,
            size,
            name,
            sample_rate: u32le(&header, 0x24),
            codec: u32le(&header, 0x28),
            checksum: u32le(&header, 0x2c),
            sections,
            metadata: None,
        })
    }

    pub(crate) fn content_start(&self) -> u64 {
        self.sections
            .iter()
            .map(|s| u64::from(s.offset) + u64::from(s.len))
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn section(&self, kind: u32) -> Result<Section, Box<dyn Error>> {
        self.sections
            .iter()
            .find(|s| s.kind == kind)
            .copied()
            .ok_or_else(|| format!("missing section {kind}").into())
    }

    pub(crate) fn diphone(
        &mut self,
        name: &str,
    ) -> Result<Option<(u32, u32, Vec<u32>)>, Box<dyn Error>> {
        let index = self.section(3)?;
        let candidates = self.section(4)?;
        if index.len % 20 != 0 || candidates.len % 4 != 0 {
            return Err("invalid diphone table dimensions".into());
        }
        self.file.seek(SeekFrom::Start(index.offset as u64))?;
        let mut bytes = vec![0u8; index.len as usize];
        self.file.read_exact(&mut bytes)?;
        let mut next = 0u32;
        let mut found = None;
        for entry in bytes.chunks_exact(20) {
            let start = u32le(entry, 12);
            let count = u32le(entry, 16);
            if start != next {
                return Err("noncontiguous diphone candidate ranges".into());
            }
            next = start.checked_add(count).ok_or("diphone range overflow")?;
            let end = entry[..12].iter().position(|&b| b == 0).unwrap_or(12);
            if &entry[..end] == name.as_bytes() {
                found = Some((start, count));
            }
        }
        if next != candidates.len / 4 {
            return Err("diphone and candidate counts disagree".into());
        }
        let Some((start, count)) = found else {
            return Ok(None);
        };
        let show = count.min(12);
        self.file
            .seek(SeekFrom::Start(candidates.offset as u64 + start as u64 * 4))?;
        let mut values = Vec::new();
        for _ in 0..show {
            let mut word = [0u8; 4];
            self.file.read_exact(&mut word)?;
            values.push(u32::from_le_bytes(word));
        }
        Ok(Some((start, count, values)))
    }

    pub(crate) fn unit_record(&mut self, index: u32) -> Result<UnitRecord, Box<dyn Error>> {
        let section = self.section(1)?;
        let count = section.len / 8;
        if section.len % 8 != 0 || index >= count {
            return Err(format!("unit index {index} outside table of {count}").into());
        }
        self.file
            .seek(SeekFrom::Start(section.offset as u64 + index as u64 * 8))?;
        let mut entry = [0u8; 8];
        self.file.read_exact(&mut entry)?;
        let offset = u32le(&entry, 0) as u64;
        let len = u32le(&entry, 4) as usize;
        if offset == 0 || len < 14 || offset + len as u64 > self.size {
            return Err(format!("unit {index} has no valid record").into());
        }
        let mut bytes = vec![0u8; len];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut bytes)?;
        let pitchmark_count = u16::from_le_bytes(bytes[0..2].try_into()?) as usize;
        if len != pitchmark_count * 2 + 12 {
            return Err(format!("unit {index} has invalid record length").into());
        }
        let pitchmarks = bytes[2..2 + pitchmark_count * 2]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let audio_offset = u32le(&bytes, len - 8) as u64;
        let encoded_bytes = u32le(&bytes, len - 4) as usize;
        if audio_offset < self.content_start()
            || encoded_bytes == 0
            || audio_offset + encoded_bytes as u64 > self.size
        {
            return Err(format!("unit {index} has invalid audio offset").into());
        }
        Ok(UnitRecord {
            pitchmarks,
            audio_offset,
            sample_skip: u16::from_le_bytes(bytes[len - 10..len - 8].try_into()?) as usize,
            encoded_bytes,
        })
    }

    pub(crate) fn candidate_ids(
        &mut self,
        name: &str,
        limit: u32,
    ) -> Result<Vec<u32>, Box<dyn Error>> {
        let index = self.section(3)?;
        let candidates = self.section(4)?;
        self.file.seek(SeekFrom::Start(index.offset as u64))?;
        let mut entries = vec![0u8; index.len as usize];
        self.file.read_exact(&mut entries)?;
        for entry in entries.chunks_exact(20) {
            let end = entry[..12].iter().position(|&b| b == 0).unwrap_or(12);
            if &entry[..end] == name.as_bytes() {
                let start = u32le(entry, 12);
                let count = u32le(entry, 16).min(limit);
                if (start as u64 + count as u64) * 4 > candidates.len as u64 {
                    return Err("diphone candidate range exceeds table".into());
                }
                self.file
                    .seek(SeekFrom::Start(candidates.offset as u64 + start as u64 * 4))?;
                let mut ids = vec![0u8; count as usize * 4];
                self.file.read_exact(&mut ids)?;
                return Ok(ids.chunks_exact(4).map(|x| u32le(x, 0)).collect());
            }
        }
        Ok(Vec::new())
    }

    pub(crate) fn unit_metadata(&mut self, id: u32) -> Result<[u8; 36], Box<dyn Error>> {
        if self.metadata.is_none() {
            let section = self.section(5)?;
            if section.len % 36 != 0 {
                return Err("invalid unit metadata section".into());
            }
            self.file.seek(SeekFrom::Start(section.offset as u64))?;
            let mut data = vec![0u8; section.len as usize];
            self.file.read_exact(&mut data)?;
            self.metadata = Some(data);
        }
        let data = self.metadata.as_ref().unwrap();
        let start = id as usize * 36;
        Ok(data
            .get(start..start + 36)
            .ok_or("unit metadata index outside section")?
            .try_into()?)
    }

    pub(crate) fn phone_code(&mut self, phone: &str) -> Result<u8, Box<dyn Error>> {
        let section = self.section(3)?;
        let candidates = self.section(4)?;
        self.file.seek(SeekFrom::Start(section.offset as u64))?;
        let mut entries = vec![0u8; section.len as usize];
        self.file.read_exact(&mut entries)?;
        for entry in entries.chunks_exact(20) {
            let end = entry[..12].iter().position(|&b| b == 0).unwrap_or(12);
            let name = std::str::from_utf8(&entry[..end])?;
            let Some((first, second)) = name.split_once('-') else {
                continue;
            };
            if u32le(entry, 16) == 0 || (first != phone && second != phone) {
                continue;
            }
            let start = u32le(entry, 12);
            self.file
                .seek(SeekFrom::Start(candidates.offset as u64 + start as u64 * 4))?;
            let mut word = [0u8; 4];
            self.file.read_exact(&mut word)?;
            let meta = self.unit_metadata(u32::from_le_bytes(word))?;
            return Ok(if first == phone { meta[4] } else { meta[5] });
        }
        Err(format!("unknown phoneme {phone}").into())
    }

    pub(crate) fn packet(&mut self, offset: u64) -> Result<(Vec<u8>, u64), Box<dyn Error>> {
        if offset + 2 > self.size {
            return Err("packet offset outside file".into());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let mut len = [0u8; 2];
        self.file.read_exact(&mut len)?;
        let n = usize::from(u16::from_le_bytes(len));
        if n == 0 || n > 0x1400 || offset + 2 + n as u64 > self.size {
            return Err(format!("invalid packet length {n} at {offset:#x}").into());
        }
        let mut payload = vec![0u8; n];
        self.file.read_exact(&mut payload)?;
        Ok((payload, offset + 2 + n as u64))
    }
}

pub(crate) fn deobfuscate(packet: &mut [u8], key: &mut [u8; 8]) {
    let rem = packet.len() & 3;
    let data_len = packet.len() - rem;
    if rem != 0 {
        key[4..4 + rem].copy_from_slice(&packet[data_len..]);
    }
    key[4 + rem..].fill(0);
    for (i, byte) in packet[..data_len].iter_mut().enumerate() {
        *byte ^= key[i % 8];
    }
}

pub(crate) struct SilkDecoder {
    state: Vec<u64>,
    control: silk_v3_sys::SKP_SILK_SDK_DecControlStruct,
}

impl SilkDecoder {
    pub(crate) fn new(rate: i32) -> Result<Self, Box<dyn Error>> {
        let mut bytes = 0i32;
        let rc = unsafe { silk_v3_sys::SKP_Silk_SDK_Get_Decoder_Size(&mut bytes) };
        if rc != 0 || bytes <= 0 {
            return Err(format!("SILK decoder size failed: {rc}").into());
        }
        let mut state = vec![0u64; (bytes as usize + 7) / 8];
        let rc =
            unsafe { silk_v3_sys::SKP_Silk_SDK_InitDecoder(state.as_mut_ptr().cast::<c_void>()) };
        if rc != 0 {
            return Err(format!("SILK decoder init failed: {rc}").into());
        }
        let mut control: silk_v3_sys::SKP_SILK_SDK_DecControlStruct = unsafe { std::mem::zeroed() };
        control.API_sampleRate = rate;
        Ok(Self { state, control })
    }

    pub(crate) fn decode_packet(
        &mut self,
        payload: &[u8],
        output: &mut Vec<i16>,
    ) -> Result<(), Box<dyn Error>> {
        let mut frame = [0i16; 9600];
        loop {
            let mut samples = frame.len() as i16;
            let rc = unsafe {
                silk_v3_sys::SKP_Silk_SDK_Decode(
                    self.state.as_mut_ptr().cast::<c_void>(),
                    &mut self.control,
                    0,
                    payload.as_ptr(),
                    payload.len() as i32,
                    frame.as_mut_ptr(),
                    &mut samples,
                )
            };
            if rc != 0 {
                return Err(format!("SILK decode failed: {rc}").into());
            }
            if samples <= 0 || samples as usize > frame.len() {
                return Err("invalid decoded sample count".into());
            }
            output.extend_from_slice(&frame[..samples as usize]);
            if self.control.moreInternalDecoderFrames == 0 {
                break;
            }
        }
        Ok(())
    }
}

/// Rate required by the SILK decoder. The rendered unit is played at the
/// VOX header rate, which can differ for codec 14.
pub(crate) fn silk_rate(db: &VoiceDb) -> u32 {
    if db.sample_rate == 0x5622 {
        24000
    } else {
        db.sample_rate
    }
}

pub(crate) fn decode_unit(db: &mut VoiceDb, index: u32) -> Result<DecodedUnit, Box<dyn Error>> {
    if db.codec != 14 {
        return Err("this decoder supports only codec 14".into());
    }
    let unit = db.unit_record(index)?;
    let wanted = unit
        .pitchmarks
        .last()
        .copied()
        .ok_or("unit has no pitchmarks")? as usize;
    let needed = unit
        .sample_skip
        .checked_add(wanted)
        .ok_or("sample count overflow")?;
    let mut decoder = SilkDecoder::new(silk_rate(db) as i32)?;
    let mut key = [0u8; 8];
    key[..4].copy_from_slice(&db.checksum.to_le_bytes());
    let mut samples = Vec::new();
    let mut pos = unit.audio_offset;
    let end = pos + unit.encoded_bytes as u64;
    while pos < end {
        let (mut packet, next) = db.packet(pos)?;
        if next > end {
            return Err(format!("unit {index} packet exceeds encoded span").into());
        }
        deobfuscate(&mut packet, &mut key);
        decoder
            .decode_packet(&packet, &mut samples)
            .map_err(|e| format!("unit {index} packet at {pos:#x}: {e}"))?;
        pos = next;
    }
    let available = samples.len().saturating_sub(unit.sample_skip).min(wanted);
    samples.resize(needed, 0);
    let mut result = samples[unit.sample_skip..needed].to_vec();
    if available < wanted && available > 0 {
        let fade = available.min(240);
        for i in 0..fade {
            let pos = available - fade + i;
            result[pos] = (result[pos] as i32 * (fade - i) as i32 / fade as i32) as i16;
        }
    }
    Ok(DecodedUnit {
        samples: result,
        pitchmarks: unit.pitchmarks,
        available,
    })
}
