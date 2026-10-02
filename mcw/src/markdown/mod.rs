//! Bounded release-highlights Markdown presentation, independent of UI and IPC.
//! This is the retained release-note grammar, not a general HTML/SVG/browser
//! engine. Unsupported markup stays literal and no resource is ever fetched.
#![forbid(unsafe_code)]
mod inline;
#[cfg(test)]
mod progress;
#[cfg(test)]
pub(crate) use progress::{ProgressCheckpoint, with_progress_observer};
use std::sync::atomic::{AtomicBool, Ordering};

pub const OPERATION: u16 = 0x1100;
pub const SCHEMA: u8 = 1;
pub const MAX_INPUT: usize = 262_144;
pub const MAX_OUTPUT: usize = 1_000_000;
pub const MAX_BLOCKS: usize = 4096;
pub const MAX_RUNS: usize = 32_768;
pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const CODE: u8 = 4;
pub const STRIKE: u8 = 8;
const MAX_WORK: usize = 8_000_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidInput,
    Limit,
    Cancelled,
}
impl Error {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid Markdown input",
            Self::Limit => "Markdown capacity exceeded",
            Self::Cancelled => "Markdown request cancelled",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Paragraph,
    Heading(u8),
    ListItem { depth: u8 },
    Code,
    Quote { depth: u8 },
    Rule,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: u8,
    pub link: Option<String>,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub kind: Kind,
    pub marker: String,
    pub runs: Vec<Run>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}
struct Budget<'a> {
    cancel: &'a AtomicBool,
    work: usize,
    runs: usize,
}
impl Budget<'_> {
    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.work = self.work.checked_add(n).ok_or(Error::Limit)?;
        if self.work > MAX_WORK {
            return Err(Error::Limit);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    #[cfg(test)]
    fn progress(&self, inline_byte_offset: usize) -> Result<(), Error> {
        if inline_byte_offset != 0 {
            progress::notify(ProgressCheckpoint {
                charged_work: self.work,
                inline_byte_offset,
            });
            // A synchronized reader can set the real token while the observer
            // is paused. Recheck it before the parser resumes this charged step.
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    }
}
pub fn parse(source: &str, cancel: &AtomicBool) -> Result<Document, Error> {
    if source.len() > MAX_INPUT {
        return Err(Error::Limit);
    }
    if source.contains('\0') {
        return Err(Error::InvalidInput);
    }
    let mut budget = Budget {
        cancel,
        work: 0,
        runs: 0,
    };
    budget.charge(source.len())?;
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized
        .strip_prefix('\u{feff}')
        .unwrap_or(&normalized)
        .split('\n')
        .collect();
    let mut references = std::collections::BTreeMap::new();
    for line in &lines {
        if let Some((name, destination, title)) = inline::reference(line, &mut budget)? {
            references.entry(name).or_insert((destination, title));
        }
    }
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut list_stack: Vec<(usize, bool, u32)> = Vec::new();
    while index < lines.len() {
        budget.charge(1)?;
        let line = lines[index];
        if line.trim().is_empty() || inline::reference(line, &mut budget)?.is_some() {
            index += 1;
            continue;
        }
        if blocks.len() >= MAX_BLOCKS {
            return Err(Error::Limit);
        }
        let (indent, trimmed) = indent(line);
        if indent <= 3 {
            if let Some((character, count, language)) = fence(trimmed) {
                let mut code = String::new();
                index += 1;
                while index < lines.len() {
                    budget.charge(lines[index].len())?;
                    let (padding, body) = self::indent(lines[index]);
                    if padding <= 3
                        && body.bytes().take_while(|b| *b == character).count() >= count
                        && body
                            .trim_start_matches(char::from(character))
                            .trim()
                            .is_empty()
                    {
                        index += 1;
                        break;
                    }
                    if !code.is_empty() {
                        code.push('\n');
                    }
                    code.push_str(lines[index]);
                    index += 1;
                }
                blocks.push(Block {
                    kind: Kind::Code,
                    marker: language.into(),
                    runs: vec![Run {
                        text: code,
                        style: CODE,
                        link: None,
                        title: None,
                    }],
                });
                list_stack.clear();
                continue;
            }
            if let Some((level, text)) = heading(trimmed) {
                blocks.push(Block {
                    kind: Kind::Heading(level),
                    marker: String::new(),
                    runs: inline::parse(text, &references, &mut budget, 0, 0, None, None)?,
                });
                index += 1;
                list_stack.clear();
                continue;
            }
            if rule(trimmed) {
                blocks.push(Block {
                    kind: Kind::Rule,
                    marker: String::new(),
                    runs: Vec::new(),
                });
                index += 1;
                list_stack.clear();
                continue;
            }
        }
        if let Some((ordered, start, content)) = list_marker(trimmed) {
            if indent > 128 {
                return Err(Error::Limit);
            }
            while list_stack.last().is_some_and(|(pad, is_ordered, _)| {
                *pad > indent || (*pad == indent && *is_ordered != ordered)
            }) {
                list_stack.pop();
            }
            let ordinal = if let Some((pad, is_ordered, number)) = list_stack
                .last_mut()
                .filter(|(pad, is_ordered, _)| *pad == indent && *is_ordered == ordered)
            {
                let _ = (pad, is_ordered);
                *number = number.checked_add(1).ok_or(Error::Limit)?;
                *number
            } else {
                list_stack.push((indent, ordered, start));
                start
            };
            if list_stack.len() > 32 {
                return Err(Error::Limit);
            }
            let depth = (list_stack.len() - 1) as u8;
            let mut text = content.to_owned();
            index += 1;
            while index < lines.len() && !lines[index].trim().is_empty() {
                let (_, next) = self::indent(lines[index]);
                if list_marker(next).is_some()
                    || heading(next).is_some()
                    || fence(next).is_some()
                    || rule(next)
                    || next.starts_with('>')
                {
                    break;
                }
                join(&mut text, next);
                index += 1;
            }
            blocks.push(Block {
                kind: Kind::ListItem { depth },
                marker: if ordered {
                    format!("{ordinal}.")
                } else {
                    "•".into()
                },
                runs: inline::parse(&text, &references, &mut budget, 0, 0, None, None)?,
            });
            continue;
        }
        list_stack.clear();
        if indent <= 3 && trimmed.starts_with('>') {
            let (depth, text) = quote(trimmed)?;
            let mut body = text.to_owned();
            index += 1;
            while index < lines.len() {
                let (padding, next) = self::indent(lines[index]);
                if padding > 3 || !next.starts_with('>') {
                    break;
                }
                let (next_depth, next) = quote(next)?;
                if next_depth != depth || next.is_empty() {
                    break;
                }
                join(&mut body, next);
                index += 1;
            }
            blocks.push(Block {
                kind: Kind::Quote { depth },
                marker: String::new(),
                runs: inline::parse(&body, &references, &mut budget, 0, 0, None, None)?,
            });
            continue;
        }
        if indent >= 4 {
            let mut code = String::new();
            while index < lines.len() {
                let (padding, body) = self::indent(lines[index]);
                if padding < 4 && !body.is_empty() {
                    break;
                }
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(
                    lines[index]
                        .strip_prefix("    ")
                        .or_else(|| lines[index].strip_prefix('\t'))
                        .unwrap_or(lines[index]),
                );
                index += 1;
            }
            blocks.push(Block {
                kind: Kind::Code,
                marker: String::new(),
                runs: vec![Run {
                    text: code.trim_end_matches('\n').into(),
                    style: CODE,
                    link: None,
                    title: None,
                }],
            });
            continue;
        }
        let mut paragraph = trimmed.to_owned();
        index += 1;
        let mut kind = Kind::Paragraph;
        while index < lines.len() && !lines[index].trim().is_empty() {
            let (padding, next) = self::indent(lines[index]);
            if let Some(level) = setext(next) {
                kind = Kind::Heading(level);
                index += 1;
                break;
            }
            if padding <= 3
                && (heading(next).is_some()
                    || fence(next).is_some()
                    || rule(next)
                    || list_marker(next).is_some()
                    || next.starts_with('>')
                    || inline::reference(next, &mut budget)?.is_some())
            {
                break;
            }
            join(&mut paragraph, next);
            index += 1;
        }
        blocks.push(Block {
            kind,
            marker: String::new(),
            runs: inline::parse(&paragraph, &references, &mut budget, 0, 0, None, None)?,
        });
    }
    let document = Document { blocks };
    if encode(&document)?.len() > MAX_OUTPUT {
        return Err(Error::Limit);
    }
    Ok(document)
}
fn indent(text: &str) -> (usize, &str) {
    let mut columns = 0;
    let mut bytes = 0;
    for ch in text.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - columns % 4,
            _ => break,
        }
        bytes += ch.len_utf8();
    }
    (columns, &text[bytes..])
}
fn heading(text: &str) -> Option<(u8, &str)> {
    let count = text.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&count)
        || text
            .as_bytes()
            .get(count)
            .is_some_and(|b| !b.is_ascii_whitespace())
    {
        return None;
    }
    let mut text = text[count..].trim();
    let body = text.trim_end_matches('#');
    if body != text && body.ends_with(char::is_whitespace) {
        text = body.trim_end();
    }
    Some((count as u8, text))
}
fn fence(text: &str) -> Option<(u8, usize, &str)> {
    let character = *text.as_bytes().first()?;
    if character != b'`' && character != b'~' {
        return None;
    }
    let count = text.bytes().take_while(|b| *b == character).count();
    if count < 3 {
        return None;
    }
    let info = text[count..].trim();
    if character == b'`' && info.contains('`') {
        return None;
    }
    Some((character, count, info))
}
fn rule(text: &str) -> bool {
    let mut chars = text.chars().filter(|ch| !ch.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '*' | '-' | '_') {
        return false;
    }
    let mut count = 1;
    for ch in chars {
        if ch != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}
fn setext(text: &str) -> Option<u8> {
    let text = text.trim_end();
    if !text.is_empty() && text.bytes().all(|b| b == b'=') {
        Some(1)
    } else if !text.is_empty() && text.bytes().all(|b| b == b'-') {
        Some(2)
    } else {
        None
    }
}
fn list_marker(text: &str) -> Option<(bool, u32, &str)> {
    let bytes = text.as_bytes();
    if bytes
        .first()
        .is_some_and(|b| matches!(b, b'-' | b'+' | b'*'))
        && bytes.get(1).is_none_or(u8::is_ascii_whitespace)
    {
        return Some((false, 0, text[1..].trim_start()));
    }
    let count = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if count == 0
        || count > 9
        || !bytes.get(count).is_some_and(|b| matches!(b, b'.' | b')'))
        || !bytes.get(count + 1).is_none_or(u8::is_ascii_whitespace)
    {
        return None;
    }
    Some((
        true,
        text[..count].parse().ok()?,
        text[count + 1..].trim_start(),
    ))
}
fn quote(mut text: &str) -> Result<(u8, &str), Error> {
    let mut depth = 0;
    while let Some(rest) = text.strip_prefix('>') {
        depth += 1;
        if depth > 32 {
            return Err(Error::Limit);
        }
        text = rest.strip_prefix(' ').unwrap_or(rest);
    }
    Ok((depth, text))
}
fn join(text: &mut String, next: &str) {
    if text.ends_with("  ") {
        let len = text.trim_end_matches(' ').len();
        text.truncate(len);
        text.push('\n');
    } else if text.ends_with('\\') {
        text.pop();
        text.push('\n');
    } else {
        text.push(' ');
    }
    text.push_str(next);
}

/// Strict schema: version/u32 block count; each block has kind/level/depth,
/// marker string/u32 run count, then style/text/link/title strings. Strings are
/// u32 UTF-8 length-prefixed. Links are inert data; adapter opens on user click.
pub fn encode(document: &Document) -> Result<Vec<u8>, Error> {
    if document.blocks.len() > MAX_BLOCKS {
        return Err(Error::Limit);
    }
    let mut result = vec![SCHEMA];
    result.extend_from_slice(&(document.blocks.len() as u32).to_le_bytes());
    let mut runs = 0;
    for block in &document.blocks {
        let (tag, level, depth) = match block.kind {
            Kind::Paragraph => (0, 0, 0),
            Kind::Heading(n) => (1, n, 0),
            Kind::ListItem { depth } => (2, 0, depth),
            Kind::Code => (3, 0, 0),
            Kind::Quote { depth } => (4, 0, depth),
            Kind::Rule => (5, 0, 0),
        };
        result.extend_from_slice(&[tag, level, depth]);
        string(&mut result, &block.marker)?;
        runs += block.runs.len();
        if runs > MAX_RUNS {
            return Err(Error::Limit);
        }
        result.extend_from_slice(&(block.runs.len() as u32).to_le_bytes());
        for run in &block.runs {
            result.push(run.style);
            string(&mut result, &run.text)?;
            string(&mut result, run.link.as_deref().unwrap_or(""))?;
            string(&mut result, run.title.as_deref().unwrap_or(""))?;
        }
    }
    Ok(result)
}
fn string(output: &mut Vec<u8>, text: &str) -> Result<(), Error> {
    if text.len() > MAX_INPUT
        || output
            .len()
            .checked_add(4 + text.len())
            .is_none_or(|n| n > MAX_OUTPUT)
    {
        return Err(Error::Limit);
    }
    output.extend_from_slice(&(text.len() as u32).to_le_bytes());
    output.extend_from_slice(text.as_bytes());
    Ok(())
}
pub fn dispatch(payload: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
    let Some((&version, source)) = payload.split_first() else {
        return Err(Error::InvalidInput);
    };
    if version != SCHEMA {
        return Err(Error::InvalidInput);
    }
    let source = std::str::from_utf8(source).map_err(|_| Error::InvalidInput)?;
    encode(&parse(source, cancel)?)
}
