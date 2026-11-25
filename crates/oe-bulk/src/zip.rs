//! Just enough of the ZIP format for the SEC's data-set archives: the central
//! directory, stored and deflated entries, CRC-32 checks. No ZIP64, no
//! encryption (neither is used by these files; both are reported as errors).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipError(pub String);

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "zip: {}", self.0)
    }
}

impl std::error::Error for ZipError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ZipError> {
    Err(ZipError(msg.into()))
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, ZipError> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| ZipError("truncated".into()))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ZipError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| ZipError("truncated".into()))
}

/// One file in the archive, as its central directory describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    local_offset: u32,
}

pub struct Archive<'a> {
    bytes: &'a [u8],
    pub entries: Vec<Entry>,
}

impl<'a> Archive<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Archive<'a>, ZipError> {
        // The end-of-central-directory record is in the last 64 KiB + 22 bytes.
        let start = bytes.len().saturating_sub(22 + 65_535);
        let eocd = (start..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .ok_or_else(|| ZipError("not a zip file (no end of central directory)".into()))?;
        let count = u16_at(bytes, eocd + 10)? as usize;
        let cd_offset = u32_at(bytes, eocd + 16)? as usize;
        if count == 0xFFFF || cd_offset == 0xFFFF_FFFF {
            return err("ZIP64 archives are not supported");
        }
        let mut entries = Vec::with_capacity(count);
        let mut at = cd_offset;
        for _ in 0..count {
            if u32_at(bytes, at)? != 0x0201_4b50 {
                return err("bad central directory entry");
            }
            let name_len = u16_at(bytes, at + 28)? as usize;
            let extra_len = u16_at(bytes, at + 30)? as usize;
            let comment_len = u16_at(bytes, at + 32)? as usize;
            let name = bytes
                .get(at + 46..at + 46 + name_len)
                .ok_or_else(|| ZipError("truncated name".into()))?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                flags: u16_at(bytes, at + 8)?,
                method: u16_at(bytes, at + 10)?,
                crc: u32_at(bytes, at + 16)?,
                compressed: u32_at(bytes, at + 20)?,
                size: u32_at(bytes, at + 24)?,
                local_offset: u32_at(bytes, at + 42)?,
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Archive { bytes, entries })
    }

    /// The entry whose file name (after any folder) is `name`, ignoring
    /// case. Whole names only: `DERIV_TRANS.tsv` must not find
    /// `NONDERIV_TRANS.tsv`.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| {
            e.name
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|f| f.eq_ignore_ascii_case(name))
        })
    }

    /// The entry's bytes, decompressed and checked against its CRC-32.
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>, ZipError> {
        if e.flags & 1 != 0 {
            return err(format!("{} is encrypted", e.name));
        }
        let at = e.local_offset as usize;
        if u32_at(self.bytes, at)? != 0x0403_4b50 {
            return err(format!("{}: bad local header", e.name));
        }
        let data_start =
            at + 30 + u16_at(self.bytes, at + 26)? as usize + u16_at(self.bytes, at + 28)? as usize;
        let data = self
            .bytes
            .get(data_start..data_start + e.compressed as usize)
            .ok_or_else(|| ZipError(format!("{}: truncated data", e.name)))?;
        let out = match e.method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data)
                .map_err(|x| ZipError(format!("{}: inflate failed ({x:?})", e.name)))?,
            m => return err(format!("{}: compression method {m} not supported", e.name)),
        };
        if out.len() != e.size as usize {
            return err(format!(
                "{}: {} bytes, expected {}",
                e.name,
                out.len(),
                e.size
            ));
        }
        if crc32fast::hash(&out) != e.crc {
            return err(format!("{}: CRC-32 mismatch", e.name));
        }
        Ok(out)
    }
}

/// For tests in this and other crates: builds a small archive by hand, each
/// file stored or deflated. Not for real archives (no zip64, no checks).
#[doc(hidden)]
pub mod tests_support {
    /// `(name, bytes, deflate)` per file.
    pub fn write(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in files {
            let body = if *deflate {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let crc = crc32fast::hash(data);
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&body);

            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0, 0, 0, 0]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn make_zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        tests_support::write(files)
    }

    #[test]
    fn reads_stored_and_deflated_entries() {
        let big = "ACCESSION_NUMBER\tX\n".repeat(500);
        let z = make_zip(&[
            ("a/SUBMISSION.tsv", big.as_bytes(), true),
            ("readme.txt", b"hello", false),
        ]);
        let a = Archive::open(&z).unwrap();
        assert_eq!(a.entries.len(), 2);
        let e = a.find("submission.TSV").unwrap();
        assert_eq!(a.read(e).unwrap(), big.as_bytes());
        assert_eq!(a.read(a.find("readme.txt").unwrap()).unwrap(), b"hello");
        assert!(a.find("missing.tsv").is_none());
        assert!(a.find("MISSION.tsv").is_none(), "whole file names only");
    }

    #[test]
    fn deriv_trans_is_not_nonderiv_trans() {
        let z = make_zip(&[
            ("NONDERIV_TRANS.tsv", b"non", false),
            ("DERIV_TRANS.tsv", b"deriv", false),
        ]);
        let a = Archive::open(&z).unwrap();
        assert_eq!(
            a.read(a.find("DERIV_TRANS.tsv").unwrap()).unwrap(),
            b"deriv"
        );
        assert_eq!(
            a.read(a.find("nonderiv_trans.tsv").unwrap()).unwrap(),
            b"non"
        );
    }

    #[test]
    fn rejects_damage() {
        assert!(Archive::open(b"not a zip at all, just text").is_err());
        let mut z = make_zip(&[("x.tsv", b"abcdefgh", false)]);
        // Flip a data byte: the CRC check must catch it.
        let pos = z.windows(8).position(|w| w == b"abcdefgh").unwrap();
        z[pos] = b'X';
        let a = Archive::open(&z).unwrap();
        assert!(a.read(&a.entries[0]).unwrap_err().0.contains("CRC"));
    }
}
