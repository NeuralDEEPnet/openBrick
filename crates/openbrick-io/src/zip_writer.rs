//! A minimal, deterministic zip writer for OPC packages such as 3MF.
//!
//! It writes what the ZIP application note (PKWARE APPNOTE.TXT 6.3.10) calls local
//! file headers, a central directory and the end of central directory record, and
//! nothing else: no data descriptors, extra fields, comments, encryption or zip64.
//! The 3MF Core specification (chapter 2, through ECMA-376 Part 2, the Open
//! Packaging Conventions) allows the stored and deflate methods, which are the two
//! used here.
//!
//! Determinism: entries are written in the order given, every timestamp is
//! 1980-01-01 00:00 (the earliest DOS date), attributes are zero, and DEFLATE is
//! miniz_oxide at a fixed level. The same entries therefore give the same bytes on
//! every platform for a given miniz_oxide version (Cargo.lock pins it).

use crate::IoError;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
/// "Version made by": 2.0, host 0 (MS-DOS and FAT, so no Unix permissions apply).
const VERSION_MADE_BY: u16 = 20;
/// "Version needed to extract": 1.0 for stored entries, 2.0 for deflate.
const VERSION_STORED: u16 = 10;
const VERSION_DEFLATE: u16 = 20;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
/// DOS date of 1980-01-01: (year - 1980) << 9 | month << 5 | day.
const DOS_DATE: u16 = 1 << 5 | 1;
const DOS_TIME: u16 = 0;
/// miniz_oxide level 6, the zlib default: a good size for the time on mesh XML.
const DEFLATE_LEVEL: u8 = 6;

/// CRC-32 lookup table for the reflected IEEE 802.3 polynomial, as zip uses.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub(crate) fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |c, &b| {
        CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// A 32-bit zip field. 0xFFFFFFFF itself is the zip64 marker, so it is excluded.
fn field32(what: &'static str, n: usize) -> Result<u32, IoError> {
    u32::try_from(n)
        .ok()
        .filter(|&v| v != u32::MAX)
        .ok_or(IoError::TooLarge {
            what,
            size: n as u64,
        })
}

/// A 16-bit zip field; 0xFFFF is the zip64 marker.
fn field16(what: &'static str, n: usize) -> Result<u16, IoError> {
    u16::try_from(n)
        .ok()
        .filter(|&v| v != u16::MAX)
        .ok_or(IoError::TooLarge {
            what,
            size: n as u64,
        })
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Write a zip archive of `(name, contents)` entries in the given order. Each entry
/// is deflated, or stored when deflating would not make it smaller. Names must be
/// ASCII (the UTF-8 flag is never set).
pub(crate) fn write_zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        debug_assert!(name.is_ascii() && crate::zip_entry_name_is_safe(name));
        let crc = crc32(data);
        let deflated = miniz_oxide::deflate::compress_to_vec(data, DEFLATE_LEVEL);
        let (method, version, payload) = if deflated.len() < data.len() {
            (METHOD_DEFLATE, VERSION_DEFLATE, deflated.as_slice())
        } else {
            (METHOD_STORED, VERSION_STORED, data)
        };
        let offset = field32("zip archive", out.len())?;
        let packed = field32("zip entry", payload.len())?;
        let size = field32("zip entry", data.len())?;
        let name_len = field16("zip entry name", name.len())?;

        // Fields shared by the local and the central header, from "general purpose
        // bit flag" to "extra field length".
        let mut common = Vec::with_capacity(26);
        put16(&mut common, version);
        put16(&mut common, 0); // flags: sizes known up front, ASCII name
        put16(&mut common, method);
        put16(&mut common, DOS_TIME);
        put16(&mut common, DOS_DATE);
        put32(&mut common, crc);
        put32(&mut common, packed);
        put32(&mut common, size);
        put16(&mut common, name_len);
        put16(&mut common, 0); // extra field length

        put32(&mut out, LOCAL_HEADER);
        out.extend_from_slice(&common);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(payload);

        put32(&mut central, CENTRAL_HEADER);
        put16(&mut central, VERSION_MADE_BY);
        central.extend_from_slice(&common);
        put16(&mut central, 0); // file comment length
        put16(&mut central, 0); // disk number start
        put16(&mut central, 0); // internal file attributes
        put32(&mut central, 0); // external file attributes
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let directory_offset = field32("zip archive", out.len())?;
    let directory_size = field32("zip central directory", central.len())?;
    let count = field16("zip entry count", entries.len())?;
    out.extend_from_slice(&central);
    put32(&mut out, END_OF_CENTRAL_DIRECTORY);
    put16(&mut out, 0); // number of this disk
    put16(&mut out, 0); // disk where the central directory starts
    put16(&mut out, count); // entries on this disk
    put16(&mut out, count); // entries in total
    put32(&mut out, directory_size);
    put32(&mut out, directory_offset);
    put16(&mut out, 0); // comment length
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        // The CRC-32/ISO-HDLC check value for the ASCII digits 1 to 9.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    fn sample() -> Vec<(&'static str, Vec<u8>)> {
        let mut long = Vec::new();
        for i in 0..2000u32 {
            long.extend_from_slice(format!("<vertex x=\"{i}\" y=\"0\" z=\"0\"/>\n").as_bytes());
        }
        vec![
            ("[Content_Types].xml", b"<Types/>".to_vec()),
            ("_rels/.rels", Vec::new()),
            ("3D/3dmodel.model", long),
        ]
    }

    fn write(entries: &[(&'static str, Vec<u8>)]) -> Vec<u8> {
        let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        write_zip(&refs).unwrap()
    }

    /// Read back with the `zip` crate, an independent implementation that checks
    /// every CRC while reading.
    #[test]
    fn an_independent_reader_gets_the_entries_back() {
        let entries = sample();
        let bytes = write(&entries);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), entries.len());
        for (i, (name, data)) in entries.iter().enumerate() {
            let mut file = archive.by_index(i).unwrap();
            assert_eq!(file.name().unwrap(), *name);
            let expected_method = if *name == "3D/3dmodel.model" {
                zip::CompressionMethod::Deflated
            } else {
                // Too short to shrink, so stored.
                zip::CompressionMethod::Stored
            };
            assert_eq!(file.compression(), expected_method, "{name}");
            let stamp = file.last_modified().expect("timestamp");
            assert_eq!(
                (
                    stamp.year(),
                    stamp.month(),
                    stamp.day(),
                    stamp.hour(),
                    stamp.minute()
                ),
                (1980, 1, 1, 0, 0)
            );
            let mut back = Vec::new();
            file.read_to_end(&mut back).unwrap();
            assert_eq!(&back, data, "{name}");
        }
    }

    #[test]
    fn output_is_byte_identical_for_identical_input() {
        let entries = sample();
        assert_eq!(write(&entries), write(&entries));
    }

    #[test]
    fn sizes_beyond_32_bit_zip_fields_are_refused() {
        assert_eq!(field32("x", 5), Ok(5));
        assert!(field32("x", u32::MAX as usize).is_err());
        assert!(field16("x", 0xFFFF).is_err());
        assert_eq!(field16("x", 0xFFFE), Ok(0xFFFE));
    }
}
