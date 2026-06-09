use std::io::Read;
use std::path::Path;

use encoding_rs_io::DecodeReaderBytesBuilder;
use memchr::memchr;

/// Outcome of reading a file with text/binary detection, per the PRD rules:
/// a UTF-16 BOM means decode-and-strip, NUL bytes mean binary, anything else
/// is loaded as UTF-8.
#[derive(Debug)]
pub enum FileContent {
    Text(String),
    Binary,
}

const CHUNK_SIZE: usize = 64 * (1 << 10);

/// Read a file, BOM-sniffing/transcoding first (so UTF-16-with-BOM arrives as
/// text) and bailing out early on the first NUL byte.
pub fn read_file(path: &Path) -> std::io::Result<FileContent> {
    let file = std::fs::File::open(path)?;
    let mut decoder = DecodeReaderBytesBuilder::new()
        .utf8_passthru(true)
        .strip_bom(true)
        .bom_override(true)
        .bom_sniffing(true)
        .build(file);

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut bytes = Vec::new();
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if memchr(0, &buf[..n]).is_some() {
            return Ok(FileContent::Binary);
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    Ok(FileContent::Text(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn read_tmp(bytes: &[u8]) -> FileContent {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        read_file(f.path()).unwrap()
    }

    fn expect_text(bytes: &[u8]) -> String {
        match read_tmp(bytes) {
            FileContent::Text(s) => s,
            FileContent::Binary => panic!("expected text"),
        }
    }

    #[test]
    fn plain_utf8_is_text() {
        assert_eq!(expect_text(b"hello\nworld\n"), "hello\nworld\n");
    }

    #[test]
    fn utf8_bom_is_stripped() {
        assert_eq!(expect_text(b"\xEF\xBB\xBFhello"), "hello");
    }

    #[test]
    fn nul_byte_means_binary() {
        assert!(matches!(read_tmp(b"abc\x00def"), FileContent::Binary));
    }

    #[test]
    fn utf16le_with_bom_is_text() {
        // "hi\n" in UTF-16LE with BOM; contains NULs but the BOM wins.
        let bytes = b"\xFF\xFEh\x00i\x00\n\x00";
        assert_eq!(expect_text(bytes), "hi\n");
    }

    #[test]
    fn utf16be_with_bom_is_text() {
        let bytes = b"\xFE\xFF\x00h\x00i";
        assert_eq!(expect_text(bytes), "hi");
    }

    #[test]
    fn bomless_utf16_is_binary_known_limitation() {
        // Documented PRD limitation: BOMless UTF-16 looks like binary.
        let bytes = b"h\x00i\x00";
        assert!(matches!(read_tmp(bytes), FileContent::Binary));
    }

    #[test]
    fn empty_file_is_text() {
        assert_eq!(expect_text(b""), "");
    }
}
