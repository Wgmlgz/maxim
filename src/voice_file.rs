use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use sha2::{Digest, Sha256};

/// A VOX is either a single original file or checked-in, directly seekable parts.
#[derive(Debug)]
pub(crate) enum VoiceFile {
    Single(File),
    Parts(PartFile),
}

impl VoiceFile {
    pub(crate) fn open(path: &Path) -> Result<(Self, u64), Box<dyn Error>> {
        if path.is_file() {
            let file = File::open(path)?;
            let size = file.metadata()?.len();
            return Ok((Self::Single(file), size));
        }
        let parts = PartFile::open(path)?;
        let size = parts.size;
        Ok((Self::Parts(parts), size))
    }
}

impl Read for VoiceFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Single(file) => file.read(buf),
            Self::Parts(file) => file.read(buf),
        }
    }
}

impl Seek for VoiceFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            Self::Single(file) => file.seek(pos),
            Self::Parts(file) => file.seek(pos),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PartFile {
    files: Vec<File>,
    starts: Vec<u64>,
    position: u64,
    size: u64,
}

fn suffixed(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

impl PartFile {
    fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        let manifest_path = suffixed(path, ".parts");
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{}: {error}", manifest_path.display()),
            )
        })?;
        let mut lines = manifest.lines();
        if lines.next() != Some("MAXIM-VOX-PARTS-1") {
            return Err(invalid("unsupported VOX parts manifest").into());
        }
        let field = |line: Option<&str>, key: &str| -> Result<String, io::Error> {
            line.and_then(|line| line.strip_prefix(key))
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("missing {key} in VOX parts manifest")))
        };
        let size: u64 = field(lines.next(), "size ")?.parse()?;
        let chunk_size: u64 = field(lines.next(), "chunk_size ")?.parse()?;
        let expected_hash = field(lines.next(), "sha256 ")?;
        if size == 0 || chunk_size == 0 || expected_hash.len() != 64 {
            return Err(invalid("invalid VOX parts manifest header").into());
        }
        let mut files = Vec::new();
        let mut starts = vec![0u64];
        let mut whole_hash = Sha256::new();
        for (index, line) in lines.enumerate() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 4 || fields[0] != "part" || fields[1] != format!("{index:03}") {
                return Err(invalid(format!("invalid VOX part entry {index}")).into());
            }
            let part_size: u64 = fields[2].parse()?;
            if part_size == 0 || part_size > chunk_size || fields[3].len() != 64 {
                return Err(invalid(format!("invalid size or hash for VOX part {index}")).into());
            }
            let part_path = suffixed(path, &format!(".part{index:03}"));
            let mut file = File::open(&part_path).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", part_path.display()))
            })?;
            if file.metadata()?.len() != part_size {
                return Err(invalid(format!("wrong size for {}", part_path.display())).into());
            }
            let mut part_hash = Sha256::new();
            let mut buffer = [0u8; 131_072];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                part_hash.update(&buffer[..count]);
                whole_hash.update(&buffer[..count]);
            }
            if hex(&part_hash.finalize()) != fields[3] {
                return Err(
                    invalid(format!("SHA-256 mismatch for {}", part_path.display())).into(),
                );
            }
            file.rewind()?;
            files.push(file);
            starts.push(
                starts
                    .last()
                    .unwrap()
                    .checked_add(part_size)
                    .ok_or("VOX size overflow")?,
            );
        }
        if files.is_empty() || *starts.last().unwrap() != size {
            return Err(invalid("VOX parts do not match manifest size").into());
        }
        if starts
            .windows(2)
            .take(files.len() - 1)
            .any(|pair| pair[1] - pair[0] != chunk_size)
        {
            return Err(invalid("non-final VOX part has wrong chunk size").into());
        }
        if hex(&whole_hash.finalize()) != expected_hash {
            return Err(invalid("SHA-256 mismatch for complete VOX").into());
        }
        Ok(Self {
            files,
            starts,
            position: 0,
            size,
        })
    }
}

impl Read for PartFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.position >= self.size {
            return Ok(0);
        }
        let index = self.starts.partition_point(|&start| start <= self.position) - 1;
        let offset = self.position - self.starts[index];
        let available = (self.starts[index + 1] - self.position) as usize;
        let file = &mut self.files[index];
        file.seek(SeekFrom::Start(offset))?;
        let requested = available.min(buf.len());
        let count = file.read(&mut buf[..requested])?;
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for PartFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(delta) => self.position as i128 + delta as i128,
            SeekFrom::End(delta) => self.size as i128 + delta as i128,
        };
        self.position =
            u64::try_from(target).map_err(|_| invalid("VOX seek outside valid range"))?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_across_part_boundaries_and_detects_damage() {
        let dir = std::env::temp_dir().join(format!(
            "maxim-parts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let base = dir.join("voice");
        let data = b"abcdefg";
        let mut manifest = format!(
            "MAXIM-VOX-PARTS-1\nsize 7\nchunk_size 4\nsha256 {}\n",
            hex(&Sha256::digest(data))
        );
        for (index, part) in [data[..4].to_vec(), data[4..].to_vec()].iter().enumerate() {
            fs::write(suffixed(&base, &format!(".part{index:03}")), part).unwrap();
            manifest.push_str(&format!(
                "part {index:03} {} {}\n",
                part.len(),
                hex(&Sha256::digest(part))
            ));
        }
        fs::write(suffixed(&base, ".parts"), manifest).unwrap();
        let mut reader = PartFile::open(&base).unwrap();
        reader.seek(SeekFrom::Start(2)).unwrap();
        let mut read = [0u8; 5];
        reader.read_exact(&mut read).unwrap();
        assert_eq!(&read, b"cdefg");
        reader.seek(SeekFrom::End(-3)).unwrap();
        reader.read_exact(&mut read[..3]).unwrap();
        assert_eq!(&read[..3], b"efg");
        fs::write(suffixed(&base, ".part001"), b"xyz").unwrap();
        assert!(PartFile::open(&base)
            .unwrap_err()
            .to_string()
            .contains("SHA-256 mismatch"));
        fs::remove_dir_all(dir).unwrap();
    }
}
