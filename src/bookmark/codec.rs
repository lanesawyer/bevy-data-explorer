//! A bookmark as a file, and as a line of text.
//!
//! The file is pretty JSON, which someone can read and a diff can show. The
//! line is the same JSON deflated and base64'd behind a prefix: short enough
//! to paste into a message, and without the newlines or quotes a chat client
//! would rewrap or curl. Reading accepts either, so pasting a file's contents
//! works as well as pasting a line.

use std::io::{Read, Write};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use super::snapshot::{Bookmark, VERSION};

/// What a shared line starts with, naming the app and the encoding's version.
const PREFIX: &str = "bde1:";

/// The most a pasted line may inflate to, so a hostile one cannot fill memory.
const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;

pub fn to_json(bookmark: &Bookmark) -> String {
    serde_json::to_string_pretty(bookmark).expect("a bookmark is always valid JSON")
}

pub fn to_line(bookmark: &Bookmark) -> String {
    let json = serde_json::to_vec(bookmark).expect("a bookmark is always valid JSON");
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(&json)
        .and_then(|()| encoder.finish())
        .map(|deflated| format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(deflated)))
        .expect("deflating into memory cannot fail")
}

/// Read a bookmark from either form.
pub fn from_text(text: &str) -> Result<Bookmark, String> {
    let text = text.trim();
    let json = match text.strip_prefix(PREFIX) {
        Some(encoded) => {
            let deflated = URL_SAFE_NO_PAD
                .decode(encoded.trim())
                .map_err(|e| format!("not a bookmark: {e}"))?;
            let mut json = Vec::new();
            DeflateDecoder::new(deflated.as_slice())
                .take(MAX_JSON_BYTES)
                .read_to_end(&mut json)
                .map_err(|e| format!("not a bookmark: {e}"))?;
            json
        }
        None if text.starts_with('{') => text.as_bytes().to_vec(),
        None => return Err("not a bookmark: expected JSON or a line starting bde1:".into()),
    };
    let bookmark: Bookmark =
        serde_json::from_slice(&json).map_err(|e| format!("not a bookmark: {e}"))?;
    if bookmark.version > VERSION {
        return Err(format!(
            "{} was saved by a newer version of the app (format {}, this reads {VERSION})",
            bookmark.name, bookmark.version
        ));
    }
    Ok(bookmark)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmark::snapshot::{FrameState, SourceState, ViewState};

    fn bookmark() -> Bookmark {
        Bookmark {
            version: VERSION,
            name: "Cortex, layer 5".into(),
            created: 1_700_000_000,
            sources: vec![SourceState {
                url: "https://example.com/a.zarr/".into(),
                slice: Some(40),
                filtered: Some(crate::source::properties::SavedFiltered {
                    shown: false,
                    color: [0.25, 0.25, 0.25],
                }),
                scale: Some(crate::source::properties::ColorScale {
                    gradient: crate::source::properties::Gradient::Turbo,
                    reversed: true,
                    whole_extent: false,
                }),
                colors: vec![crate::source::properties::SavedColor {
                    column: "class".into(),
                    code: 3,
                    color: [1.0, 0.5, 0.0],
                }],
                ..Default::default()
            }],
            frames: vec![FrameState {
                source: 0,
                view: Some(ViewState {
                    center: [1.0, 2.0],
                    extent: [300.0, 200.0],
                }),
                orbit: None,
                layers: Vec::new(),
                linked: true,
                selection: Some(crate::bookmark::snapshot::RegionState {
                    min: [-4.5, -20.0],
                    max: [12.0, -3.25],
                    focus: Some(crate::bookmark::snapshot::FocusState {
                        column: "class".into(),
                        label: "04 DG-IMN Glut".into(),
                    }),
                }),
            }],
            selected: Some(0),
            empty_frames: Vec::new(),
        }
    }

    #[test]
    fn both_forms_read_back_as_what_was_written() {
        let bookmark = bookmark();
        assert_eq!(from_text(&to_json(&bookmark)), Ok(bookmark.clone()));
        assert_eq!(from_text(&to_line(&bookmark)), Ok(bookmark));
    }

    #[test]
    fn a_bookmark_written_before_selections_still_reads() {
        // Raising the version refuses every older link, so a field merely
        // added has to read as absent instead.
        let mut value: serde_json::Value = serde_json::from_str(&to_json(&bookmark())).unwrap();
        value["frames"][0]
            .as_object_mut()
            .unwrap()
            .remove("selection");
        let read = from_text(&value.to_string()).expect("an older bookmark still reads");
        assert_eq!(read.frames[0].selection, None);
        assert_eq!(read.version, VERSION);
    }

    #[test]
    fn a_line_is_one_line_of_safe_characters() {
        let line = to_line(&bookmark());
        assert!(line.starts_with(PREFIX));
        assert!(
            line[PREFIX.len()..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn whitespace_round_a_pasted_line_is_ignored() {
        let line = format!("  {}\n", to_line(&bookmark()));
        assert!(from_text(&line).is_ok());
    }

    #[test]
    fn a_newer_format_is_refused_by_name() {
        let mut newer = bookmark();
        newer.version = VERSION + 1;
        let error = from_text(&to_json(&newer)).unwrap_err();
        assert!(error.contains("newer"));
    }

    #[test]
    fn text_that_is_not_a_bookmark_says_so() {
        assert!(from_text("https://example.com/a.zarr").is_err());
        assert!(from_text("bde1:!!!").is_err());
        assert!(from_text("{}").is_err());
    }
}
