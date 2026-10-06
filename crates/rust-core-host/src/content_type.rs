//! Content-type detection by magic bytes.
//!
//! Used to (a) route changed files — text goes to the semantic parser, binary /
//! image assets go to the perceptual asset diff — and (b) enrich every diff's
//! metadata with the detected MIME type and category. Detection reads only the
//! leading bytes of a file (real content sniffing, not the filename extension).
//!
//! `infer` is tried first (zero-dependency, fast, no `unsafe` in the matcher);
//! `file-format` is a broader fallback; a UTF-8 heuristic is the final arbiter
//! for text. Never panics — unknown content is reported as
//! `application/octet-stream`.

use serde::Serialize;

/// Privacy-safe: only the MIME/category/flags, never file content.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContentType {
    /// e.g. `"image/png"`, `"text/plain"`, `"application/octet-stream"`.
    pub mime: String,
    /// Canonical extension for the detected type, or `""` when unknown.
    pub extension: String,
    /// Coarse bucket: `text` | `image` | `video` | `audio` | `archive` | `font`
    /// | `document` | `executable` | `binary` | `empty`.
    pub category: String,
    /// Whether the file should be sent to the semantic *text* engine.
    pub is_text: bool,
}

impl ContentType {
    fn text() -> Self {
        Self { mime: "text/plain".into(), extension: String::new(), category: "text".into(), is_text: true }
    }

    fn binary_unknown() -> Self {
        Self { mime: "application/octet-stream".into(), extension: String::new(), category: "binary".into(), is_text: false }
    }
}

/// Detect the content type from the leading bytes of a file.
pub const HEAD_BYTES: usize = 8192;

/// Inspect at most HEAD_BYTES; every binding and native caller uses this window.
pub fn detect_content_type(head: &[u8]) -> ContentType {
    let head = &head[..head.len().min(HEAD_BYTES)];
    if head.is_empty() {
        return ContentType { mime: "inode/x-empty".into(), extension: String::new(), category: "empty".into(), is_text: true };
    }

    // 1. `infer` recognizes both binary signatures and source text signatures.
    if let Some(kind) = infer::get(head) {
        let is_text = recognized_text(kind.mime_type(), head);
        return ContentType {
            mime: kind.mime_type().to_string(),
            extension: kind.extension().to_string(),
            category: if is_text { "text".into() } else { category_from_infer(kind.matcher_type()) },
            is_text,
        };
    }

    // 2. `file-format` — broader coverage for formats infer misses.
    let fmt = file_format::FileFormat::from_bytes(head);
    if fmt == file_format::FileFormat::PlainText {
        return ContentType::text();
    }
    if fmt != file_format::FileFormat::ArbitraryBinaryData {
        let is_text = recognized_text(fmt.media_type(), head);
        return ContentType {
            mime: fmt.media_type().to_string(),
            extension: fmt.extension().to_string(),
            category: if is_text { "text".into() } else { category_from_file_format(fmt.kind()) },
            is_text,
        };
    }

    // 3. UTF-8 heuristic — the final text/binary arbiter.
    if looks_like_text(head) {
        ContentType::text()
    } else {
        ContentType::binary_unknown()
    }
}

/// Recognizing a MIME type must not exclude source from Git review. PostScript
/// is a supported source language although sniffers classify its MIME as an
/// application/archive. Keep known binary signatures authoritative (even an
/// ASCII-compatible PDF/GIF header) and reject NUL/invalid UTF-8 text payloads.
fn recognized_text(mime: &str, head: &[u8]) -> bool {
    (mime.starts_with("text/") || mime == "application/postscript") && looks_like_text(head)
}

/// A leading slice is text when it has no NUL byte and is valid UTF-8 (allowing a
/// single multi-byte character truncated at the end of the sampled window).
fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // A truncated final code point (no explicit error length) still looks text.
        Err(err) => err.error_len().is_none() && err.valid_up_to() > 0,
    }
}

fn category_from_infer(matcher: infer::MatcherType) -> String {
    use infer::MatcherType as M;
    match matcher {
        M::Image => "image",
        M::Video => "video",
        M::Audio => "audio",
        M::Archive => "archive",
        M::Font => "font",
        M::Doc | M::Book => "document",
        M::App => "executable",
        M::Text => "text",
        M::Custom => "binary",
    }
    .to_string()
}

fn category_from_file_format(kind: file_format::Kind) -> String {
    use file_format::Kind as K;
    match kind {
        K::Image => "image",
        K::Video => "video",
        K::Audio => "audio",
        K::Archive | K::Compressed | K::Package | K::Disk | K::Rom => "archive",
        K::Font => "font",
        K::Document | K::Ebook | K::Presentation | K::Spreadsheet => "document",
        K::Executable => "executable",
        _ => "binary",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_detected_as_a_binary_image() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        let ct = detect_content_type(png);
        assert_eq!(ct.mime, "image/png");
        assert_eq!(ct.category, "image");
        assert!(!ct.is_text);
    }

    #[test]
    fn jpeg_and_gif_are_images() {
        assert_eq!(detect_content_type(b"\xFF\xD8\xFF\xE0").mime, "image/jpeg");
        assert_eq!(detect_content_type(b"GIF89a").category, "image");
    }

    #[test]
    fn pdf_and_zip_are_binary() {
        let pdf = detect_content_type(b"%PDF-1.7\n");
        assert!(!pdf.is_text);
        assert_eq!(pdf.mime, "application/pdf");
        let zip = detect_content_type(b"PK\x03\x04");
        assert_eq!(zip.category, "archive");
        assert!(!zip.is_text);
    }

    #[test]
    fn utf8_source_is_text() {
        let ct = detect_content_type(b"def foo():\n    return 1\n");
        assert!(ct.is_text);
        assert_eq!(ct.mime, "text/plain");
        assert_eq!(ct.category, "text");
    }

    #[test]
    fn nul_bytes_mark_binary() {
        let ct = detect_content_type(b"some text\x00then a nul");
        assert!(!ct.is_text);
        assert_eq!(ct.category, "binary");
    }

    #[test]
    fn empty_is_treated_as_text() {
        let ct = detect_content_type(b"");
        assert!(ct.is_text);
        assert_eq!(ct.category, "empty");
    }

    #[test]
    fn truncated_multibyte_tail_is_still_text() {
        // "é" is 0xC3 0xA9; drop the final byte to simulate a truncated window.
        let ct = detect_content_type(b"caf\xc3");
        assert!(ct.is_text);
    }

    #[test]
    fn recognized_source_formats_still_route_to_text() {
        for source in [
            "#!/bin/bash\necho hello\n",
            "<!DOCTYPE html>\n<html><title>Hello</title></html>\n",
            "<?xml version=\"1.0\"?><root>Hello</root>\n",
            "<script>let name = 'World';</script>\n<h1>{name}</h1>\n",
            "(module (func (result i32) i32.const 1))\n",
            "%!PS\n/Helvetica findfont 12 scalefont setfont\n(Hello) show\nshowpage\n",
        ] {
            let ct = detect_content_type(source.as_bytes());
            assert!(ct.is_text, "source was excluded: {ct:?}");
            assert_eq!(ct.category, "text");
        }
    }

    #[test]
    fn recognized_text_must_still_pass_byte_safety_checks() {
        for source in [b"#!/bin/bash\necho\x00bad".as_slice(), b"<html>\xff</html>", b"%!PS\n\x00"] {
            assert!(!detect_content_type(source).is_text);
        }
        // ASCII-compatible binary signatures must not become source text.
        for source in [b"%PDF-1.7\n".as_slice(), b"GIF89a", b"PK\x03\x04", b"\0asm\x01\0\0\0"] {
            assert!(!detect_content_type(source).is_text);
        }
    }
}
