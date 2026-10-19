//! The media kinds a package carries: one table of content types and file-name extensions that
//! both directions read (`mime_for` for parts coming in, `ext_for` for parts going out), so the
//! two cannot drift apart.

/// One media kind.
struct MediaKind {
    /// The content type written into `[Content_Types].xml` and part headers.
    mime: &'static str,
    /// The part-name extension it is written as; empty for kinds that are only ever read.
    write_ext: &'static str,
    /// Extensions [`mime_for`] reads as this kind.
    exts: &'static [&'static str],
    /// Other content-type spellings [`ext_for`] accepts for this kind.
    mime_aliases: &'static [&'static str],
}

/// Every kind of media a package carries.
const MEDIA: &[MediaKind] = &[
    MediaKind { mime: "image/png", write_ext: "png", exts: &["png"], mime_aliases: &[] },
    MediaKind { mime: "image/jpeg", write_ext: "jpeg", exts: &["jpg", "jpeg", "jpe"], mime_aliases: &["image/jpg"] },
    MediaKind { mime: "image/gif", write_ext: "gif", exts: &["gif"], mime_aliases: &[] },
    MediaKind { mime: "image/bmp", write_ext: "bmp", exts: &["bmp"], mime_aliases: &[] },
    MediaKind { mime: "image/tiff", write_ext: "tiff", exts: &["tif", "tiff"], mime_aliases: &[] },
    MediaKind { mime: "image/svg+xml", write_ext: "svg", exts: &["svg"], mime_aliases: &[] },
    MediaKind { mime: "image/x-emf", write_ext: "emf", exts: &["emf"], mime_aliases: &["image/emf"] },
    MediaKind { mime: "image/x-wmf", write_ext: "wmf", exts: &["wmf"], mime_aliases: &["image/wmf"] },
    MediaKind { mime: "image/webp", write_ext: "webp", exts: &["webp"], mime_aliases: &[] },
    MediaKind { mime: "video/mp4", write_ext: "mp4", exts: &["mp4", "m4v"], mime_aliases: &[] },
    MediaKind { mime: "video/quicktime", write_ext: "mov", exts: &["mov"], mime_aliases: &[] },
    MediaKind { mime: "video/x-ms-wmv", write_ext: "wmv", exts: &["wmv"], mime_aliases: &[] },
    MediaKind { mime: "video/x-msvideo", write_ext: "avi", exts: &["avi"], mime_aliases: &[] },
    MediaKind { mime: "video/webm", write_ext: "webm", exts: &["webm"], mime_aliases: &[] },
    MediaKind { mime: "audio/mpeg", write_ext: "mp3", exts: &["mp3"], mime_aliases: &[] },
    MediaKind { mime: "audio/wav", write_ext: "wav", exts: &["wav"], mime_aliases: &["audio/x-wav", "audio/wave"] },
    MediaKind { mime: "audio/mp4", write_ext: "m4a", exts: &["m4a"], mime_aliases: &["audio/x-m4a"] },
    MediaKind { mime: "audio/x-ms-wma", write_ext: "wma", exts: &["wma"], mime_aliases: &[] },
    MediaKind { mime: "audio/ogg", write_ext: "ogg", exts: &["ogg"], mime_aliases: &[] },
    MediaKind { mime: "audio/flac", write_ext: "flac", exts: &["flac"], mime_aliases: &[] },
    MediaKind { mime: "model/gltf-binary", write_ext: "glb", exts: &["glb"], mime_aliases: &[] },
    // Read-only for now: embedded workbooks and font data go by the type the file declares.
    MediaKind { mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", write_ext: "", exts: &["xlsx"], mime_aliases: &[] },
    MediaKind { mime: "application/x-fontdata", write_ext: "", exts: &["fntdata"], mime_aliases: &[] },
];

/// Content type for a file name's extension.
pub(crate) fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    MEDIA.iter().find(|m| m.exts.contains(&ext.as_str())).map_or("application/octet-stream", |m| m.mime)
}

/// The file-name extension written for a media item: from its content type when the table knows
/// one, else sniffed from its first bytes, else the name's own extension, else `bin`.
pub(crate) fn ext_for(content_type: &str, name: &str, data: &[u8]) -> String {
    if let Some(m) = MEDIA.iter().find(|m| m.mime == content_type || m.mime_aliases.contains(&content_type))
        && !m.write_ext.is_empty()
    {
        return m.write_ext.to_string();
    }
    if data.starts_with(b"\x89PNG") {
        return "png".into();
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        return "jpeg".into();
    }
    if data.starts_with(b"GIF8") {
        return "gif".into();
    }
    let e = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()) { e } else { "bin".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_written_kind_reads_back_as_itself() {
        for m in MEDIA {
            if m.write_ext.is_empty() {
                continue;
            }
            assert_eq!(mime_for(&format!("x.{}", m.write_ext)), m.mime, "reading {}", m.mime);
            assert_eq!(ext_for(m.mime, "x", b""), m.write_ext, "writing {}", m.mime);
        }
    }

    #[test]
    fn every_extension_and_spelling_reads_as_its_kind() {
        for m in MEDIA {
            for ext in m.exts {
                assert_eq!(mime_for(&format!("file.{ext}")), m.mime, "{ext}");
                assert_eq!(mime_for(&format!("file.{}", ext.to_ascii_uppercase())), m.mime, "{ext}, upper-case");
            }
            for alias in m.mime_aliases {
                assert_eq!(ext_for(alias, "x", b""), m.write_ext, "{alias}");
            }
        }
    }

    #[test]
    fn unknown_types_are_named_then_binned() {
        assert_eq!(mime_for("no-extension"), "application/octet-stream");
        assert_eq!(mime_for("a.unknown"), "application/octet-stream");
        assert_eq!(ext_for("", "a.zz", b"not an image"), "zz");
        assert_eq!(ext_for("", "no-extension", b""), "bin");
        // A long or odd extension is not trusted.
        assert_eq!(ext_for("", "a.too-long", b""), "bin");
        assert_eq!(ext_for("", "a.h4x!", b""), "bin");
        // Font data has no extension written for it: the name's own (short) one is used.
        assert_eq!(ext_for("application/x-fontdata", "a.odttf", b""), "odttf");
        assert_eq!(ext_for("application/x-fontdata", "a.fntdata", b""), "bin");
    }

    #[test]
    fn untyped_data_is_sniffed() {
        assert_eq!(ext_for("", "x", b"\x89PNG\r\n"), "png");
        assert_eq!(ext_for("", "x", b"\xFF\xD8\xFF"), "jpeg");
        assert_eq!(ext_for("", "x", b"GIF89a"), "gif");
    }
}
