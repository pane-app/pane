//! A small Markdown parser for the designed tree's `markdown` node (#237,
//! ADR 0036): enough of CommonMark for documentation — headings,
//! paragraphs, fenced and indented code, block quotes, ordered and
//! unordered lists with task-list checkboxes, thematic breaks, inline
//! emphasis, strong emphasis, code, links and images, and GitHub's
//! tables — read into typed blocks the host window draws with its own
//! theme. LaTeX is out of scope, and so is the rest of CommonMark: no
//! reference links, no footnotes, no HTML (a designed tree shows anything
//! an image cannot with its own nodes; the Detail view is where Markdown
//! images land, #240).
//!
//! An image reads `![alt](source "title")`, its source the icon model's
//! (a packaged image's path, a web image's or an inline `data:` URL),
//! with an optional size between the source and the title:
//! `![alt](source =100x50)` (either number may be left out). The parser
//! keeps the source as written; the icon it reads as is resolved by the
//! host when the tree lands, as any icon of the tree is (`visit_images`).
//!
//! The parser is hand-written, deliberately: the tree's Markdown is a
//! UI component's content, bounded by the document's limits, and adding
//! a Markdown dependency to pane-core for it would weigh more than this
//! file. What it does not understand it leaves as text, the way a lenient
//! renderer degrades, and its reading is total: `parse` never fails, so a
//! Markdown node never makes a tree unreadable.

use crate::icons::Icon;

/// One block of a Markdown document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// An ATX heading, `#` to `######`.
    Heading { level: usize, inlines: Vec<Inline> },
    /// A run of lines of prose.
    Paragraph(Vec<Inline>),
    /// A fenced or indented code block, its `language` when the fence
    /// names one.
    Code {
        language: Option<String>,
        text: String,
    },
    /// A block quote, quoting further blocks.
    Quote(Vec<Block>),
    /// An ordered or unordered list. An item's first block may carry a
    /// task's checkbox.
    List { ordered: bool, items: Vec<Item> },
    /// A thematic break: `---`, `***` or `___`.
    Rule,
    /// A GitHub table: its columns' alignments, its head row and its body
    /// rows.
    Table {
        aligns: Vec<Alignment>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
}

/// One item of a list, its `checked` checkbox when it is a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// One run of inline content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    /// Prose.
    Text(String),
    /// Inline code: `` `code` ``.
    Code(String),
    /// Emphasis: `*like this*`.
    Emphasis(Vec<Inline>),
    /// Strong emphasis: `**like this**`.
    Strong(Vec<Inline>),
    /// A link: `[text](href)`. The host draws it as a link, not as prose;
    /// the standard views open what it names (#240).
    Link { text: Vec<Inline>, href: String },
    /// An image: `![alt](source "title")`, its source the icon model's,
    /// with an optional size (`=100x50`, either number may be left out).
    /// `icon` is the source read as the icon model reads one, resolved in
    /// the open command's package folder by the host as any icon of the
    /// tree is; the parser leaves it `None`, and the drawing falls back to
    /// the source as written until it is resolved.
    Image {
        alt: String,
        source: String,
        title: Option<String>,
        /// Its width in pixels, when the image's size names one.
        width: Option<u32>,
        /// Its height in pixels, when the image's size names one.
        height: Option<u32>,
        /// The source as the icon model reads it, resolved by the host;
        /// `None` until the tree that holds it lands.
        icon: Option<Icon>,
    },
    /// A hard line break: two spaces at a line's end.
    Break,
}

/// Visits every image the blocks hold — the icon its source reads as,
/// which the host resolves in the open command's package folder and
/// starts the loads of, and the source itself — so Markdown images ride
/// the icon machinery the tree's own icons do (#240).
pub fn visit_images(blocks: &mut [Block], visit: &mut dyn FnMut(&mut Option<Icon>, &str)) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph(inlines) => {
                visit_inline_images(inlines, visit)
            }
            Block::Quote(inner) => visit_images(inner, visit),
            Block::List { items, .. } => {
                for item in items {
                    visit_images(&mut item.blocks, visit)
                }
            }
            Block::Table { head, rows, .. } => {
                for cell in head {
                    visit_inline_images(cell, visit)
                }
                for row in rows {
                    for cell in row {
                        visit_inline_images(cell, visit)
                    }
                }
            }
            Block::Code { .. } | Block::Rule => {}
        }
    }
}

/// The images of one run of inline content.
fn visit_inline_images(inlines: &mut [Inline], visit: &mut dyn FnMut(&mut Option<Icon>, &str)) {
    for inline in inlines {
        match inline {
            Inline::Emphasis(inner) | Inline::Strong(inner) => visit_inline_images(inner, visit),
            Inline::Link { text: inner, .. } => visit_inline_images(inner, visit),
            Inline::Image { source, icon, .. } => visit(icon, source),
            Inline::Text(_) | Inline::Code(_) | Inline::Break => {}
        }
    }
}

/// Visits every resolved image the blocks hold, as they are: whose loads
/// the host starts and whose arrivals it shows (#240).
pub fn each_image(blocks: &[Block], visit: &mut dyn FnMut(&Icon)) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph(inlines) => {
                each_inline_image(inlines, visit)
            }
            Block::Quote(inner) => each_image(inner, visit),
            Block::List { items, .. } => {
                for item in items {
                    each_image(&item.blocks, visit)
                }
            }
            Block::Table { head, rows, .. } => {
                for cell in head {
                    each_inline_image(cell, visit)
                }
                for row in rows {
                    for cell in row {
                        each_inline_image(cell, visit)
                    }
                }
            }
            Block::Code { .. } | Block::Rule => {}
        }
    }
}

/// The resolved images of one run of inline content.
fn each_inline_image(inlines: &[Inline], visit: &mut dyn FnMut(&Icon)) {
    for inline in inlines {
        match inline {
            Inline::Emphasis(inner) | Inline::Strong(inner) => each_inline_image(inner, visit),
            Inline::Link { text: inner, .. } => each_inline_image(inner, visit),
            Inline::Image { icon, .. } => {
                if let Some(icon) = icon {
                    visit(icon)
                }
            }
            Inline::Text(_) | Inline::Code(_) | Inline::Break => {}
        }
    }
}

/// One column's alignment in a table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// `markdown` as blocks. Total: whatever cannot be read is text.
pub fn parse(markdown: &str) -> Vec<Block> {
    blocks(&mut Lines::of(markdown))
}

/// The lines of a document, walking them block by block.
struct Lines<'a> {
    lines: Vec<&'a str>,
    at: usize,
}

impl<'a> Lines<'a> {
    fn of(markdown: &'a str) -> Lines<'a> {
        Lines {
            lines: markdown.lines().collect(),
            at: 0,
        }
    }

    /// The next line, without taking it.
    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.at).copied()
    }

    /// The line `further` after the next, without taking any.
    fn after(&self, further: usize) -> Option<&'a str> {
        self.lines.get(self.at + further).copied()
    }
}

/// The blocks of the lines `lines` holds.
fn blocks(lines: &mut Lines) -> Vec<Block> {
    let mut parsed = Vec::new();
    while let Some(line) = lines.peek() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            lines.at += 1;
        } else if let Some((tilde, language)) = fence(trimmed) {
            // A fenced code block: the lines up to a fence that closes it.
            lines.at += 1;
            let text = code_until(tilde, lines);
            parsed.push(Block::Code {
                language,
                text: text.join("\n"),
            });
        } else if let Some(block) = heading(trimmed) {
            lines.at += 1;
            parsed.push(block);
        } else if rule(trimmed) {
            lines.at += 1;
            parsed.push(Block::Rule);
        } else if trimmed.starts_with('>') {
            let quoted = lines
                .lines
                .get(lines.at..)
                .unwrap_or(&[])
                .iter()
                .take_while(|line| line.trim_start().starts_with('>'))
                .map(|line| strip_quote(line))
                .collect::<Vec<&str>>()
                .join("\n");
            let quoted_lines = quoted.lines().count();
            parsed.push(Block::Quote(parse(&quoted)));
            lines.at += quoted_lines;
        } else if list_marker(trimmed).is_some() {
            parsed.push(list(lines));
        } else if let Some(table) = table(lines) {
            parsed.push(table);
        } else if indent >= 4 {
            // An indented code block: no paragraph has begun, so four
            // spaces open one.
            let code = lines
                .lines
                .get(lines.at..)
                .unwrap_or(&[])
                .iter()
                .take_while(|line| line.starts_with("    ") || line.trim().is_empty())
                .map(|line| &line[4.min(line.len())..])
                .collect::<Vec<&str>>();
            lines.at += code.len();
            parsed.push(Block::Code {
                language: None,
                text: code.join("\n").trim_end().to_owned(),
            });
        } else {
            // A paragraph: the lines until one starts another block.
            let paragraph = lines
                .lines
                .get(lines.at..)
                .unwrap_or(&[])
                .iter()
                .take_while(|line| {
                    let trimmed = line.trim_start();
                    (line.len() - trimmed.len()) < 4
                        && !trimmed.is_empty()
                        && fence(trimmed).is_none()
                        && heading(trimmed).is_none()
                        && !rule(trimmed)
                        && !trimmed.starts_with('>')
                        && list_marker(trimmed).is_none()
                        && !table_start(line)
                })
                .copied()
                .collect::<Vec<&str>>();
            let length = paragraph.len();
            parsed.push(Block::Paragraph(inlines(&paragraph.join("\n"))));
            lines.at += length;
        }
    }
    parsed
}

/// `line` as a fence's opening: ` ``` ` or `~~~`, with an optional
/// language after it — with or without a space between, as CommonMark
/// writes an info string. The bool says the fence is a tilde one.
fn fence(line: &str) -> Option<(bool, Option<String>)> {
    let tilde = line.starts_with("~~~");
    let rest = if tilde {
        &line[3..]
    } else {
        line.strip_prefix("```")?
    };
    // The language: the fence's info string, trimmed of the space that
    // may sit before it. Only whitespace follows the language — a
    // fence's line names nothing else.
    let language = rest.trim();
    if language.is_empty() {
        return Some((tilde, None));
    }
    (!language.is_empty() && language == language.trim())
        .then(|| (tilde, Some(language.to_owned())))
}

/// `line` as a heading, else `None`.
fn heading(line: &str) -> Option<Block> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    let text = rest.strip_prefix(' ').unwrap_or(rest);
    // The closing hashes of a heading, if any, go with it.
    let text = text.trim_end_matches('#').trim_end();
    Some(Block::Heading {
        level: hashes,
        inlines: inlines(text),
    })
}

/// Whether `line` is a thematic break.
fn rule(line: &str) -> bool {
    let Some(kind) = line.chars().next() else {
        return false;
    };
    if !matches!(kind, '-' | '*' | '_') {
        return false;
    }
    let rest = &line[1..];
    let marks = rest.chars().filter(|c| *c == kind).count() + 1;
    marks >= 3 && rest.chars().all(|c| c == kind || c == ' ')
}

/// The lines of the fenced code block that `lines` holds, up to a fence
/// of the same kind closing it, taking them.
fn code_until(tilde: bool, lines: &mut Lines) -> Vec<String> {
    let mark = if tilde { '~' } else { '`' };
    let closes = |line: &str| {
        let trimmed = line.trim_start();
        trimmed.len() >= 3 && trimmed.chars().take_while(|c| *c == mark).count() >= 3
    };
    let code = lines
        .lines
        .get(lines.at..)
        .unwrap_or(&[])
        .iter()
        .take_while(|line| !closes(line))
        .map(|line| (*line).to_owned())
        .collect::<Vec<String>>();
    lines.at += code.len();
    // The closing fence goes with it, when the document has one.
    if lines.peek().is_some_and(closes) {
        lines.at += 1;
    }
    code
}

/// `line` with its block quote's `>` marker taken off.
fn strip_quote(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix('>')
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        .unwrap_or(trimmed)
}

/// The marker a list line starts with: `None` when it is not one. The
/// bool says the list is ordered, the number how wide the marker is.
fn list_marker(line: &str) -> Option<(bool, usize)> {
    let bullet = |mark: char| {
        line.starts_with(mark)
            .then(|| {
                line[1..]
                    .chars()
                    .next()
                    .filter(|after| *after == ' ' || *after == '\t')
                    .map(|_| (false, 2))
            })
            .flatten()
    };
    if let Some(ordered) = bullet('-').or_else(|| bullet('*')).or_else(|| bullet('+')) {
        return Some(ordered);
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && line
            .get(digits + 1..digits + 2)
            .is_some_and(|after| after == " " || after == "\t")
    {
        return Some((true, digits + 2));
    }
    None
}

/// The list the lines at `lines` hold, taking them.
fn list(lines: &mut Lines) -> Block {
    let (ordered, width) = list_marker(lines.peek().expect("a list line")).expect("a marker");
    let mut items = Vec::new();
    while let Some(line) = lines.peek() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some(_) = list_marker(trimmed) else {
            break;
        };
        let _ = indent;
        // The item's own line, then the lines that belong to it: deeper
        // indented ones, and lazy continuations of its paragraph.
        let mut own = vec![&trimmed[width..]];
        lines.at += 1;
        while let Some(next) = lines.peek() {
            let next_trimmed = next.trim_start();
            let next_indent = next.len() - next_trimmed.len();
            if next_trimmed.is_empty() {
                // A blank line ends the item unless a deeper line follows.
                let follows = lines.after(1).unwrap_or("");
                let follows_indent = follows.len() - follows.trim_start().len();
                if follows.trim().is_empty() || follows_indent < 2 {
                    break;
                }
                own.push("");
                lines.at += 1;
                continue;
            }
            if next_indent >= 2 {
                // An item's content: the marker's width is stripped, as
                // the item's first line had it stripped.
                let strip = width.min(next_indent);
                own.push(&next[strip..]);
                lines.at += 1;
                continue;
            }
            if list_marker(next_trimmed).is_some() {
                break;
            }
            // A lazy continuation: prose directly under the item's, with
            // no blank line between.
            if own.last().is_some_and(|last| !last.trim().is_empty()) {
                own.push(next_trimmed);
                lines.at += 1;
            } else {
                break;
            }
        }
        let (checked, blocks) = task(&own.join("\n"));
        items.push(Item { checked, blocks });
    }
    Block::List { ordered, items }
}

/// `text` as a task item: its checkbox and the item's blocks. The
/// checkbox `[ ]` or `[x]` opens the item.
fn task(text: &str) -> (Option<bool>, Vec<Block>) {
    let checked = if text.starts_with("[ ] ") {
        Some(false)
    } else if text.starts_with("[x] ") || text.starts_with("[X] ") {
        Some(true)
    } else {
        None
    };
    match checked {
        Some(_) => (checked, parse(&text[4..])),
        None => (None, parse(text)),
    }
}

/// Whether a table starts at `line`: a row of cells with a delimiter row
/// after it.
fn table_start(line: &str) -> bool {
    line.contains('|') && delimiter_row(line).is_some()
}

/// `line` as a table's delimiter row, of alignments, when it is one.
fn delimiter_row(line: &str) -> Option<Vec<Alignment>> {
    let trimmed = line.trim();
    let cells = cells_of(trimmed);
    let mut aligns = Vec::new();
    for cell in cells {
        let cell = cell.as_str();
        let align = if let Some(rest) = cell.strip_prefix(':') {
            if rest
                .strip_suffix(':')
                .is_some_and(|dashes| dashes.chars().all(|c| c == '-'))
            {
                Alignment::Center
            } else if rest.chars().all(|c| c == '-') && !rest.is_empty() {
                Alignment::Right
            } else {
                return None;
            }
        } else if cell
            .strip_suffix(':')
            .is_some_and(|dashes| dashes.chars().all(|c| c == '-'))
        {
            Alignment::Left
        } else if !cell.is_empty() && cell.chars().all(|c| c == '-') {
            Alignment::None
        } else {
            return None;
        };
        aligns.push(align);
    }
    (!aligns.is_empty()).then_some(aligns)
}

/// The table the lines at `lines` hold, taking them, when one starts
/// there.
fn table(lines: &mut Lines) -> Option<Block> {
    let head_line = lines.peek().expect("a row");
    let columns = cells_of(head_line).len();
    let aligns = delimiter_row(lines.after(1)?)?;
    if aligns.len() != columns {
        return None;
    }
    let head = cells_of(head_line)
        .into_iter()
        .map(|cell| inlines(&cell))
        .collect::<Vec<_>>();
    let rows = lines
        .lines
        .get(lines.at + 2..)
        .unwrap_or(&[])
        .iter()
        .take_while(|line| !line.trim().is_empty() && line.contains('|'))
        .map(|line| {
            let mut cells = cells_of(line);
            cells.resize(columns, String::new());
            cells
                .into_iter()
                .map(|cell| inlines(&cell))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    lines.at += 2 + rows.len();
    Some(Block::Table { aligns, head, rows })
}

/// The cells of one table `line`, split on unescaped pipes, its own edge
/// pipes taken off and each cell trimmed.
fn cells_of(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed
        .strip_prefix('|')
        .map(|rest| rest.strip_suffix('|').unwrap_or(rest))
        .unwrap_or(trimmed);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in trimmed.chars() {
        match (escaped, character) {
            (true, _) => {
                cell.push(character);
                escaped = false;
            }
            (false, '\\') => escaped = true,
            // The cell ends here: what follows the pipe is the next one,
            // not a continuation of this.
            (false, '|') => cells.push(std::mem::take(&mut cell)),
            _ => cell.push(character),
        }
    }
    cells.push(cell);
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// `text` as inline runs: emphasis, strong emphasis, code and links.
fn inlines(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while at < chars.len() {
        let character = chars[at];
        // A hard break: two spaces before a newline.
        if character == '\n' && plain.ends_with("  ") {
            plain.truncate(plain.len() - 2);
            push_text(&mut runs, &plain);
            plain.clear();
            runs.push(Inline::Break);
            at += 1;
            continue;
        }
        if character == '\n' {
            plain.push(' ');
            at += 1;
            continue;
        }
        if character == '\\' && at + 1 < chars.len() && "*_`[]".contains(chars[at + 1]) {
            plain.push(chars[at + 1]);
            at += 2;
            continue;
        }
        // Code: the shortest run of backticks with one after it.
        if character == '`' {
            let ticks = chars[at..].iter().take_while(|c| **c == '`').count();
            if let Some(close) = find_run(&chars, at + ticks, '`', ticks) {
                let code: String = chars[at + ticks..close].iter().collect();
                push_text(&mut runs, &plain);
                plain.clear();
                runs.push(Inline::Code(code.trim().to_owned()));
                at = close + ticks;
                continue;
            }
        }
        // A link: `[text](href)`, or an image: `![alt](source …)`.
        if (character == '[' || (character == '!' && chars.get(at + 1) == Some(&'[')))
            && let Some((inner, inside, close)) = linked(&chars, at)
        {
            push_text(&mut runs, &plain);
            plain.clear();
            if character == '!' {
                runs.push(image(&inner, &inside));
            } else {
                runs.push(Inline::Link {
                    text: inlines(&inner),
                    href: inside.trim().to_owned(),
                });
            }
            at = close + 1;
            continue;
        }
        // Emphasis and strong emphasis: a run of one or two markers.
        if character == '*' || character == '_' {
            let run = chars[at..].iter().take_while(|c| **c == character).count();
            let width = run.min(2);
            if let Some(close) = find_run(&chars, at + width, character, width)
                && emphasized(&chars, at, close + width, character)
            {
                let inner: String = chars[at + width..close].iter().collect();
                push_text(&mut runs, &plain);
                plain.clear();
                let inner = inlines(&inner);
                runs.push(if width == 2 {
                    Inline::Strong(inner)
                } else {
                    Inline::Emphasis(inner)
                });
                at = close + width;
                continue;
            }
        }
        plain.push(character);
        at += 1;
    }
    push_text(&mut runs, &plain);
    runs
}

/// Whether an emphasis run between `open` and `close` stands at word
/// boundaries: `_` only emphasizes whole words (`snake_case` is a name),
/// while `*` has no such rule.
fn emphasized(chars: &[char], open: usize, close: usize, character: char) -> bool {
    if character != '_' {
        return true;
    }
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    !word(chars.get(open.wrapping_sub(1))) && !word(chars.get(close))
}

/// The bracketed link or image starting at `at`: its inner text, the
/// content of its parentheses, and the index of its closing parenthesis;
/// `None` when there is no `]…)` run there. An image starts `![`, a link
/// `[`.
fn linked(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let start = at + usize::from(chars[at] == '!');
    let end = find_plain(chars, start + 1, ']')?;
    if chars.get(end + 1) != Some(&'(') {
        return None;
    }
    let close = find_plain(chars, end + 2, ')')?;
    let inner: String = chars[start + 1..end].iter().collect();
    let inside: String = chars[end + 2..close].iter().collect();
    Some((inner, inside, close))
}

/// One image: its `alt` text, and the content of its parentheses — its
/// source, an optional `=WxH` size after it and an optional quoted title
/// after that, either of which may be given alone.
fn image(alt: &str, inside: &str) -> Inline {
    let mut held = inside.trim();
    let mut title = None;
    if let (Some(open), Some(close)) = (held.find('"'), held.rfind('"'))
        && open < close
    {
        title = Some(held[open + 1..close].to_owned());
        held = held[..open].trim_end();
    }
    let mut source = held;
    let mut width = None;
    let mut height = None;
    if let Some(at) = held.find(" =")
        && let Some((given_width, given_height)) = size_of(held[at + 2..].trim())
    {
        width = given_width;
        height = given_height;
        source = held[..at].trim_end();
    }
    Inline::Image {
        alt: alt.to_owned(),
        source: source.to_owned(),
        title,
        width,
        height,
        icon: None,
    }
}

/// `100x50`, `100x` or `x50` as the width and height it names, when it
/// spells one of them.
fn size_of(text: &str) -> Option<(Option<u32>, Option<u32>)> {
    let (width, height) = text.split_once('x')?;
    if width.is_empty() && height.is_empty() {
        return None;
    }
    let read = |part: &str| {
        (!part.is_empty())
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    if (!width.is_empty() && read(width).is_none())
        || (!height.is_empty() && read(height).is_none())
    {
        return None;
    }
    Some((read(width), read(height)))
}

/// `text` as one run of prose, when it says anything.
fn push_text(runs: &mut Vec<Inline>, text: &str) {
    if !text.is_empty() {
        runs.push(Inline::Text(text.to_owned()));
    }
}

/// The first index at or after `from` of a run of `character` exactly
/// `width` long, escaped characters skipped.
fn find_run(chars: &[char], from: usize, character: char, width: usize) -> Option<usize> {
    let mut at = from;
    while at < chars.len() {
        if chars[at] == '\\' {
            at += 2;
            continue;
        }
        if chars[at] == character {
            let run = chars[at..].iter().take_while(|c| **c == character).count();
            if run == width {
                return Some(at);
            }
            at += run;
            continue;
        }
        at += 1;
    }
    None
}

/// The first index at or after `from` of `character`, unescaped.
fn find_plain(chars: &[char], from: usize, character: char) -> Option<usize> {
    let mut at = from;
    while at < chars.len() {
        match chars[at] {
            found if found == character => return Some(at),
            '\\' => at += 1,
            _ => {}
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `markdown`'s paragraphs and headings, flattened to their text.
    fn text_of(markdown: &str) -> Vec<String> {
        parse(markdown)
            .into_iter()
            .map(|block| match block {
                Block::Paragraph(runs) | Block::Heading { inlines: runs, .. } => runs
                    .into_iter()
                    .filter(|run| matches!(run, Inline::Text(_)))
                    .map(|run| match run {
                        Inline::Text(text) => text,
                        other => format!("{other:?}"),
                    })
                    .collect::<String>(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn paragraphs_headings_and_breaks_read() {
        assert_eq!(
            text_of("One paragraph\nover two lines."),
            vec!["One paragraph over two lines."]
        );
        assert_eq!(text_of("## A heading"), vec!["A heading"]);
        let blocks = parse("A hard  \nbreak.");
        let Block::Paragraph(runs) = &blocks[0] else {
            panic!("a paragraph: {blocks:?}");
        };
        assert!(runs.contains(&Inline::Break), "{runs:?}");
    }

    #[test]
    fn emphasis_code_and_links_read() {
        let blocks = parse("Some *emphasis*, **strong**, `code` and [a link](https://pane.dev).");
        let Block::Paragraph(runs) = &blocks[0] else {
            panic!("a paragraph: {blocks:?}");
        };
        assert!(
            runs.contains(&Inline::Emphasis(vec![Inline::Text("emphasis".into())])),
            "{runs:?}"
        );
        assert!(
            runs.contains(&Inline::Strong(vec![Inline::Text("strong".into())])),
            "{runs:?}"
        );
        assert!(runs.contains(&Inline::Code("code".into())), "{runs:?}");
        assert!(
            runs.contains(&Inline::Link {
                text: vec![Inline::Text("a link".into())],
                href: "https://pane.dev".into()
            }),
            "{runs:?}"
        );
        // An underscore inside a word does not emphasize.
        let Block::Paragraph(runs) = &parse("snake_case_name")[0] else {
            panic!("a paragraph");
        };
        assert_eq!(
            runs.as_slice(),
            &[Inline::Text("snake_case_name".into())],
            "{runs:?}"
        );
    }

    #[test]
    fn lists_and_tasks_read() {
        let blocks = parse("- one\n- [x] two\n- [ ] three");
        let Block::List { ordered, items } = &blocks[0] else {
            panic!("a list: {blocks:?}");
        };
        assert!(!ordered);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].checked, None);
        assert_eq!(items[1].checked, Some(true));
        assert_eq!(items[2].checked, Some(false));
        let blocks = parse("2. first\n3. second");
        let Block::List { ordered, items } = &blocks[0] else {
            panic!("a list: {blocks:?}");
        };
        assert!(ordered);
        assert_eq!(items.len(), 2);
        // An item's continuation line belongs to it.
        let blocks = parse("- one\n  and two");
        let Block::List { items, .. } = &blocks[0] else {
            panic!("a list: {blocks:?}");
        };
        assert_eq!(text_of("one and two\n"), vec!["one and two"]);
        let Block::Paragraph(runs) = &items[0].blocks[0] else {
            panic!("a paragraph: {:?}", items[0].blocks);
        };
        assert_eq!(
            runs.as_slice(),
            &[Inline::Text("one and two".into())],
            "{runs:?}"
        );
    }

    #[test]
    fn quotes_rules_and_code_read() {
        let blocks = parse("> quoted *prose*");
        let Block::Quote(inner) = &blocks[0] else {
            panic!("a quote: {blocks:?}");
        };
        assert!(matches!(&inner[0], Block::Paragraph(_)), "{inner:?}");
        assert!(matches!(parse("---")[0], Block::Rule));
        let blocks = parse("```rust\nfn main() {}\n```");
        let Block::Code { language, text } = &blocks[0] else {
            panic!("code: {blocks:?}");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "fn main() {}");
        let blocks = parse("    indented code");
        assert!(
            matches!(&blocks[0], Block::Code { language: None, .. }),
            "{blocks:?}"
        );
    }

    #[test]
    fn images_read_with_their_sizes_and_titles() {
        let blocks = parse(
            "![Pane's mark](assets/mark.png =48x24 \"the mark\") and ![icon](https://pane.dev/logo.png).",
        );
        let Block::Paragraph(runs) = &blocks[0] else {
            panic!("a paragraph: {blocks:?}");
        };
        let Inline::Image {
            alt,
            source,
            title,
            width,
            height,
            icon,
        } = &runs[0]
        else {
            panic!("an image: {runs:?}");
        };
        assert_eq!(alt, "Pane's mark");
        assert_eq!(source, "assets/mark.png");
        assert_eq!(title.as_deref(), Some("the mark"));
        assert_eq!((*width, *height), (Some(48), Some(24)));
        assert!(icon.is_none());
        // A web image, and a size with one number left out.
        let Inline::Image { source, .. } = &runs[2] else {
            panic!("an image: {runs:?}");
        };
        assert_eq!(source, "https://pane.dev/logo.png");
        let blocks = parse("![half](half.png =100x)");
        let Block::Paragraph(runs) = &blocks[0] else {
            panic!("a paragraph");
        };
        let Inline::Image { width, height, .. } = &runs[0] else {
            panic!("an image: {runs:?}");
        };
        assert_eq!((*width, *height), (Some(100), None));
    }

    #[test]
    fn tables_read() {
        let blocks = parse("| A | B |\n| --- | :---: |\n| 1 | 2 |");
        let Block::Table { aligns, head, rows } = &blocks[0] else {
            panic!("a table: {blocks:?}");
        };
        assert_eq!(*aligns, vec![Alignment::None, Alignment::Center]);
        assert_eq!(head.len(), 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 2);
    }
}
