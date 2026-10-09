//! Source maps of JavaScript and TypeScript development builds: mapping
//! the positions of a stack trace a command throws back to the sources its
//! bundle was built from (#128's user story 24, #214).
//!
//! A development build keeps its map beside the component
//! ([`SourceMap::beside`]); the runtime reads it when it starts the
//! component's instance and maps what the package writes to its standard
//! output and error as the lines are captured, so the Logs screen, the
//! development log file, the log's followers and the error overlay all show
//! frames that name the author's sources. Only the frames' positions are
//! mapped: a stack a Rust command's trap carries is a wasm backtrace, and
//! a JavaScript crash (a guest answer of the wrong type) has no stack of
//! its own.
//!
//! The map is read as the source map v3 format: `sources` and `mappings`,
//! the mappings base64 VLQ encoded. Nothing else of the format is needed
//! (the build keeps no sources content, so the map stays small), and no
//! crate is added for it: the VLQ decoding is the few lines below.

use std::path::Path;

/// What the bundle a command was built from is called where it runs: the
/// componentizer names the module after the file it was given, which
/// Pane's build calls `bundle.mjs` (with the `!/` prefix of the
/// component's module names before it). Frames of a stack name it, as in
/// `at run (!/bundle.mjs:12:3)`; matching the file's name alone, and not
/// the whole module name, keeps the mapping working whatever path the
/// build put the bundle at.
const MODULE: &str = "bundle.mjs";

/// The map of one component's bundle: the sources it was built from, and
/// where each position of the bundle came from.
#[derive(Debug)]
pub(crate) struct SourceMap {
    /// The bundle's sources, as the map names them: paths relative to the
    /// bundle, the package's own files and the SDK's under their folders.
    sources: Vec<String>,
    /// For each line of the bundle (0-based), its mapping segments, in
    /// generated-column order.
    lines: Vec<Vec<Segment>>,
}

/// Where a range of one generated line of the bundle came from.
#[derive(Debug)]
struct Segment {
    /// The generated column the segment starts at, 0-based.
    column: u32,
    /// The source (its index, and the line and column in it, 0-based) the
    /// segment maps to; `None` where the bundle maps to no source.
    source: Option<(u32, u32, u32)>,
}

impl SourceMap {
    /// The map of the component at `component`, when one is kept beside it
    /// (its file name plus `.map`): a development build of JavaScript or
    /// TypeScript put it there, and the reload carried it with the
    /// component. `None` for a component with no map, a Rust one or one
    /// built for release.
    pub(crate) fn beside(component: &Path) -> Option<SourceMap> {
        let name = component.file_name()?.to_str()?;
        SourceMap::read(&component.with_file_name(format!("{name}.map")))
    }

    /// The map in the file at `path`, if it is one: read and parsed. A
    /// map that is missing or cannot be parsed maps nothing, silently: it
    /// is a convenience of development, never a promise to the author.
    pub(crate) fn read(path: &Path) -> Option<SourceMap> {
        let text = std::fs::read_to_string(path).ok()?;
        SourceMap::parse(&text)
    }

    /// The map `text` holds: its `sources` and its `mappings`. Positions
    /// the mappings cannot make sense of are skipped, as the format allows
    /// extensions the map may carry.
    fn parse(text: &str) -> Option<SourceMap> {
        let map: serde_json::Value = serde_json::from_str(text).ok()?;
        let sources: Vec<String> = map
            .get("sources")?
            .as_array()?
            .iter()
            .map(|source| source.as_str().unwrap_or_default().to_owned())
            .collect();
        let mappings = map.get("mappings")?.as_str()?;
        Some(SourceMap {
            sources,
            lines: parse_mappings(mappings),
        })
    }

    /// `text`, with the stack frames it holds mapped back to the sources
    /// they were built from: every `bundle.mjs:line:column` that maps is
    /// replaced by `source:line:column`, and everything else, a frame that
    /// maps nowhere included, is left as it is. Lines the package writes
    /// that hold no frames of the bundle come back as they were.
    pub(crate) fn map_frames(&self, text: &str) -> String {
        if !text.contains(MODULE) {
            return text.to_owned();
        }
        let needle = format!("{MODULE}:");
        let mut mapped = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(&needle) {
            // What follows the module's name: a frame's position, if this
            // is one. Whatever it is, the name and the position travel
            // together from here.
            let after = &rest[at + needle.len()..];
            match position(after) {
                Some((line, column, taken)) => {
                    let end = at + needle.len() + taken;
                    match self.at(line, column) {
                        Some((source, line, column)) => {
                            mapped.push_str(&rest[..at]);
                            mapped.push_str(source);
                            mapped.push_str(&format!(":{line}:{column}"));
                        }
                        // A position the map has nothing for keeps the
                        // bundle's.
                        None => mapped.push_str(&rest[..end]),
                    }
                    rest = &rest[end..];
                }
                None => {
                    // The module's name without a position after it: not a
                    // frame; keep it and look on.
                    let end = at + needle.len();
                    mapped.push_str(&rest[..end]);
                    rest = &rest[end..];
                }
            }
        }
        mapped.push_str(rest);
        mapped
    }

    /// The source and position the bundle's `line` and `column` (both as a
    /// stack frame names them, 1-based) were built from, if the map has
    /// one: the last mapping segment at or before the column on that line,
    /// as the format's segments cover what follows them.
    fn at(&self, line: u32, column: u32) -> Option<(&str, u32, u32)> {
        let segments = self.lines.get(line.checked_sub(1)? as usize)?;
        let column = column.saturating_sub(1);
        let last = segments.partition_point(|segment| segment.column <= column);
        let found = segments.get(last.checked_sub(1)?)?;
        let (source, line, column) = found.source?;
        Some((
            self.sources.get(source as usize)?.as_str(),
            line + 1,
            column + 1,
        ))
    }
}

/// The position a stack frame names after `bundle.mjs:` — `line:column`,
/// both 1-based — and how many bytes it takes; `None` when the text
/// following the name is not one.
fn position(after: &str) -> Option<(u32, u32, usize)> {
    /// `text`'s leading digits, and how many bytes they take.
    fn digits(text: &str) -> Option<(u32, usize)> {
        let taken = text.bytes().take_while(u8::is_ascii_digit).count();
        if taken == 0 {
            return None;
        }
        Some((text[..taken].parse().ok()?, taken))
    }
    let (line, line_taken) = digits(after)?;
    if after.as_bytes().get(line_taken) != Some(&b':') {
        return None;
    }
    let (column, column_taken) = digits(after.get(line_taken + 1..)?)?;
    Some((line, column, line_taken + 1 + column_taken))
}

/// The map's `mappings`: one `;`-separated group per generated line, each
/// holding `,`-separated segments of base64 VLQ fields — the generated
/// column, then, for a segment that maps, the source, its line and its
/// column, all deltas from the segment before (the generated column from
/// the line's start, the rest from the map's). A segment of one field maps
/// to nothing, and fields no segment has a use for are ignored.
fn parse_mappings(mappings: &str) -> Vec<Vec<Segment>> {
    // Where the last segment that mapped left the source, its line and its
    // column, which the next segment that maps is a delta from.
    let mut carried = (0i64, 0i64, 0i64);
    let mut lines = Vec::new();
    for generated in mappings.split(';') {
        let mut segments = Vec::new();
        // The generated column, which each segment of the line is a delta
        // from the one before it.
        let mut at = 0i64;
        for segment in generated.split(',') {
            if segment.is_empty() {
                continue;
            }
            let Some(fields) = vlqs(segment) else {
                continue;
            };
            at = (at + fields.first().copied().unwrap_or_default()).clamp(0, u32::MAX as i64);
            let source = match fields.as_slice() {
                [_, source, line, column, ..] => {
                    carried = (
                        (carried.0.wrapping_add(*source)).clamp(0, u32::MAX as i64),
                        (carried.1.wrapping_add(*line)).clamp(0, u32::MAX as i64),
                        (carried.2.wrapping_add(*column)).clamp(0, u32::MAX as i64),
                    );
                    Some((carried.0 as u32, carried.1 as u32, carried.2 as u32))
                }
                // A segment that maps nowhere ends what the one before it
                // covered.
                _ => None,
            };
            segments.push(Segment {
                column: at as u32,
                source,
            });
        }
        lines.push(segments);
    }
    lines
}

/// The base64 VLQ fields of one segment of the mappings, in order; `None`
/// for a segment that is not whole. Each field is a signed integer: 5 bits
/// at a time, least significant first, the high bit of each 6-bit digit
/// saying another follows, and the field's own low bit its sign.
fn vlqs(segment: &str) -> Option<Vec<i64>> {
    let mut values = Vec::new();
    let (mut value, mut shift) = (0u32, 0);
    let mut started = false;
    for byte in segment.bytes() {
        let digit = base64(byte)?;
        value |= u32::from(digit & 0x1f) << shift;
        started = true;
        if digit & 0x20 == 0 {
            let negative = value & 1 != 0;
            let magnitude = i64::from(value >> 1);
            values.push(if negative { -magnitude } else { magnitude });
            (value, shift, started) = (0, 0, false);
        } else {
            shift += 5;
            if shift > 25 {
                return None;
            }
        }
    }
    (!started).then_some(values)
}

/// The 6-bit value a base64 digit of the mappings stands for.
fn base64(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `values` in the mappings' base64 VLQ, as the build's tools write
    /// them: the tests build their maps with it, so they say what they
    /// mean rather than spell encodings out.
    fn encoded(values: &[i64]) -> String {
        const DIGITS: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::new();
        for value in values {
            let mut bits = (value.unsigned_abs() << 1) | u64::from(*value < 0);
            loop {
                let mut digit = (bits & 0x1f) as u8;
                bits >>= 5;
                if bits > 0 {
                    digit |= 0x20;
                }
                encoded.push(DIGITS[digit as usize] as char);
                if bits == 0 {
                    break;
                }
            }
        }
        encoded
    }

    /// A map whose bundle has the given segments per generated line
    /// (0-based): each a `(column, Some((source, line, column)))` with the
    /// generated column 0-based and the source's line and column 0-based,
    /// as the mappings carry them; `None` where a segment maps nowhere.
    fn map(lines: &[&[(u32, Option<(u32, u32, u32)>)]], sources: &[&str]) -> SourceMap {
        let mut mappings = String::new();
        // Where the last segment that mapped left the source, its line and
        // its column.
        let mut carried = (0i64, 0i64, 0i64);
        for (line, segments) in lines.iter().enumerate() {
            if line > 0 {
                mappings.push(';');
            }
            let mut at = 0i64;
            for (column, source) in segments.iter().copied() {
                if !mappings.is_empty() && !mappings.ends_with([';', ',']) {
                    mappings.push(',');
                }
                let mut fields = vec![i64::from(column) - at];
                at = i64::from(column);
                if let Some((source, line, column)) = source {
                    fields.extend([
                        i64::from(source) - carried.0,
                        i64::from(line) - carried.1,
                        i64::from(column) - carried.2,
                    ]);
                    carried = (source.into(), line.into(), column.into());
                }
                mappings.push_str(&encoded(&fields));
            }
        }
        SourceMap::parse(&format!(
            r#"{{"version":3,"sources":{},"mappings":{}}}"#,
            serde_json::to_string(sources).unwrap(),
            serde_json::to_string(&mappings).unwrap()
        ))
        .expect("the map parses")
    }

    #[test]
    fn vlq_round_trips() {
        for values in [
            vec![0],
            vec![1],
            vec![-1],
            vec![4, 9, 20, -3, 2047, -1024],
            vec![i64::from(u32::MAX), -i64::from(u32::MAX)],
        ] {
            assert_eq!(vlqs(&encoded(&values)), Some(values.clone()), "{values:?}");
        }
        // A digit left hanging carries no value.
        assert_eq!(vlqs("g"), None);
    }

    #[test]
    fn frames_map_to_their_sources() {
        // The bundle's second line: its first columns from the package's
        // index.ts, from column 10 on from the SDK's adapter, and from
        // column 40 nowhere.
        let source = map(
            &[
                &[],
                &[(0, Some((0, 11, 2))), (10, Some((1, 29, 0))), (40, None)],
            ],
            &["pkg/src/index.ts", "js/adapt.js"],
        );
        let trace = "Error: failed on purpose\n\
                     \x20   at run (bundle.mjs:2:1)\n\
                     \x20   at adapt (!/bundle.mjs:2:11)\n\
                     \x20   at beyond (bundle.mjs:2:41)\n";
        assert_eq!(
            source.map_frames(trace),
            "Error: failed on purpose\n\
             \x20   at run (pkg/src/index.ts:12:3)\n\
             \x20   at adapt (js/adapt.js:30:1)\n\
             \x20   at beyond (bundle.mjs:2:41)\n"
        );
    }

    #[test]
    fn a_frame_between_segments_takes_the_one_before_it() {
        // A frame's column lands inside a segment: the segment that
        // started before it owns it, as the format's segments cover what
        // follows them.
        let source = map(&[&[(0, Some((0, 4, 0)))]], &["a.ts"]);
        assert_eq!(
            source.map_frames("    at fn (bundle.mjs:1:9)"),
            "    at fn (a.ts:5:1)"
        );
    }

    #[test]
    fn lines_without_the_bundle_are_left_alone() {
        let source = map(&[&[(0, Some((0, 0, 0)))]], &["a.ts"]);
        for text in [
            "a plain line",
            "src/index.ts:12:3 already names a source",
            "the bundle.mjs without a position",
            "bundle.mjs:x:y is not a position",
            "bundle.mjs:12 not a position either",
        ] {
            assert_eq!(source.map_frames(text), text);
        }
    }

    #[test]
    fn positions_the_map_has_nothing_for_stay_the_bundle_s() {
        // A second line the map says nothing of, and the line 0 a frame
        // never names.
        let source = map(&[&[], &[]], &["a.ts"]);
        for text in ["    at fn (bundle.mjs:2:1)", "    at fn (bundle.mjs:0:1)"] {
            assert_eq!(source.map_frames(text), text);
        }
    }
}
