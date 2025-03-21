/// Group bytes into u16 values for conversion to UTF-16, respecting
/// the byte order mark if present.
fn u16_from_bytes(bytes: &[u8]) -> Vec<u16> {
    let is_big_endian = match &bytes {
        [0xfe, 0xff, ..] => true,
        [0xff, 0xfe, ..] => false,
        _ => false, // assume little endian if no BOM is present.
    };

    // https://stackoverflow.com/a/57172592
    bytes
        .chunks_exact(2)
        .map(|a| {
            if is_big_endian {
                u16::from_be_bytes([a[0], a[1]])
            } else {
                u16::from_le_bytes([a[0], a[1]])
            }
        })
        .collect()
}

fn has_utf16_byte_order_mark(bytes: &[u8]) -> bool {
    matches!(bytes, [0xfe, 0xff, ..] | [0xff, 0xfe, ..])
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProbableFileKind {
    Text(String),
    Binary,
}

/// Do these bytes look like a binary (non-textual) format?
pub fn guess_content(bytes: &[u8]) -> ProbableFileKind {
    // If the bytes are entirely valid UTF-8, treat them as a string.
    if let Ok(valid_utf8_string) = std::str::from_utf8(bytes) {
        return ProbableFileKind::Text(valid_utf8_string.to_string());
    }

    // Only consider the first 1,000 bytes, as tree_magic_mini
    // considers the entire file, which is very slow on large files.
    let mut magic_bytes = bytes;
    if magic_bytes.len() > 1000 {
        magic_bytes = &magic_bytes[..1000];
    }

    let mime = tree_magic_mini::from_u8(magic_bytes);

    // Use MIME type detection to guess whether a file is binary. This
    // has false positives and false negatives, so only check the MIME
    // type after allowing perfect text files (see issue #433).
    match mime {
        // Treat pdf as binary.
        "application/pdf" => return ProbableFileKind::Binary,
        // application/* is a mix of stuff, application/json is fine
        // but application/zip is binary that often decodes as valid
        // UTF-16.
        "application/gzip" => return ProbableFileKind::Binary,
        "application/zip" => return ProbableFileKind::Binary,
        // Treat all image content as binary.
        v if v.starts_with("image/") => return ProbableFileKind::Binary,
        // Treat all audio content as binary.
        v if v.starts_with("audio/") => return ProbableFileKind::Binary,
        // Treat all video content as binary.
        v if v.starts_with("video/") => return ProbableFileKind::Binary,
        // Treat all font content as binary.
        v if v.starts_with("font/") => return ProbableFileKind::Binary,
        _ => {}
    }

    // Note that many binary files and mostly-valid UTF-8 files happen
    // to be valid UTF-16. Decoding these as UTF-16 leads to garbage
    // ("mojibake").
    //
    // To avoid this, we only try UTF-16 after we'vedone MIME type
    // checks for binary, and we conservatively require an explicit
    // byte order mark.
    let u16_values = u16_from_bytes(bytes);
    let utf16_str_result = String::from_utf16(&u16_values);
    match utf16_str_result {
        Ok(valid_utf16_string) if has_utf16_byte_order_mark(bytes) => {
            return ProbableFileKind::Text(valid_utf16_string);
        }
        _ => {}
    }

    // If the input bytes are *almost* valid UTF-8, treat them as UTF-8.
    let utf8_string = String::from_utf8_lossy(bytes).to_string();
    let num_utf8_invalid = utf8_string
        .chars()
        .take(5000)
        .filter(|c| *c == std::char::REPLACEMENT_CHARACTER)
        .count();
    if num_utf8_invalid <= 10 {
        return ProbableFileKind::Text(utf8_string);
    }

    // If the input bytes are *almost* valid UTF-16, treat them as
    // UTF-16.
    let utf16_string = String::from_utf16_lossy(&u16_values);
    let num_utf16_invalid = utf16_string
        .chars()
        .take(5000)
        .filter(|c| *c == std::char::REPLACEMENT_CHARACTER)
        .count();
    if num_utf16_invalid <= 5 {
        return ProbableFileKind::Text(utf16_string);
    }

    ProbableFileKind::Binary
}
