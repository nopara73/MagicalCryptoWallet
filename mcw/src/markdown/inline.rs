use super::{BOLD, Budget, CODE, Error, ITALIC, MAX_RUNS, Run, STRIKE};
use std::collections::BTreeMap;
type References = BTreeMap<String, (String, Option<String>)>;
type Reference = (String, String, Option<String>);
pub(super) fn reference(line: &str, budget: &mut Budget<'_>) -> Result<Option<Reference>, Error> {
    // Reference parsing is linear in this one line. Reserve its work before
    // trimming, searching or allocating; callers may inspect a line again.
    budget.charge(line.len())?;
    Ok(reference_content(line))
}
fn reference_content(line: &str) -> Option<Reference> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let end = rest.find("]:")?;
    let name = key(&rest[..end]);
    if name.is_empty() {
        return None;
    }
    let (destination, title, consumed) = destination(rest[end + 2..].trim(), false)?;
    if !rest[end + 2..].trim()[consumed..].trim().is_empty() {
        return None;
    }
    Some((name, destination, title))
}
fn scan(
    bytes: &[u8],
    limit: usize,
    budget: &mut Budget<'_>,
    matches: impl Fn(u8) -> bool,
) -> Result<Option<usize>, Error> {
    // Cap the slice BEFORE searching. Every chunk is reserved and cancellation
    // checked BEFORE inspecting it, including searches with no terminator.
    let window = &bytes[..bytes.len().min(limit)];
    for (index, chunk) in window.chunks(64).enumerate() {
        budget.charge(chunk.len())?;
        if let Some(offset) = chunk.iter().position(|byte| matches(*byte)) {
            return Ok(Some(index * 64 + offset));
        }
    }
    Ok(None)
}
fn find_byte(
    bytes: &[u8],
    byte: u8,
    limit: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<usize>, Error> {
    scan(bytes, limit, budget, |candidate| candidate == byte)
}
fn run_length(bytes: &[u8], byte: u8, budget: &mut Budget<'_>) -> Result<usize, Error> {
    Ok(scan(bytes, bytes.len(), budget, |candidate| candidate != byte)?.unwrap_or(bytes.len()))
}
fn key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn reference_target(
    references: &References,
    name: &str,
    budget: &mut Budget<'_>,
) -> Result<Option<(String, Option<String>)>, Error> {
    let Some((destination, title)) = references.get(&key(name)) else {
        return Ok(None);
    };
    budget.charge(destination.len() + title.as_deref().map_or(0, str::len))?;
    Ok(Some((destination.clone(), title.clone())))
}
pub(super) fn safe_link(text: &str) -> bool {
    if text.len() > 4096
        || text.chars().any(|c| c.is_control() || c.is_whitespace())
        || text.contains('\\')
    {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        return !authority.is_empty() && !authority.starts_with('.') && !authority.contains('@');
    }
    lower
        .strip_prefix("mailto:")
        .is_some_and(|mail| mail.contains('@') && !mail.starts_with('@') && !mail.ends_with('@'))
}
fn push(
    output: &mut Vec<Run>,
    text: &str,
    style: u8,
    link: Option<&str>,
    title: Option<&str>,
    budget: &mut Budget<'_>,
) -> Result<(), Error> {
    if text.is_empty() {
        return Ok(());
    }
    // Metadata can be much larger than its label. Reserve comparisons and
    // copies before merging or allocating, not just the visible text bytes.
    budget.charge(text.len() + link.map_or(0, str::len) + title.map_or(0, str::len))?;
    if let Some(last) = output
        .last_mut()
        .filter(|r| r.style == style && r.link.as_deref() == link && r.title.as_deref() == title)
    {
        last.text.push_str(text);
        return Ok(());
    }
    budget.runs += 1;
    if budget.runs > MAX_RUNS {
        return Err(Error::Limit);
    }
    output.push(Run {
        text: text.into(),
        style,
        link: link.map(str::to_owned),
        title: title.map(str::to_owned),
    });
    Ok(())
}
pub(super) fn parse(
    text: &str,
    references: &References,
    budget: &mut Budget<'_>,
    depth: u8,
    style: u8,
    link: Option<&str>,
    title: Option<&str>,
) -> Result<Vec<Run>, Error> {
    if depth > 32 {
        return Err(Error::Limit);
    }
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        budget.charge(1)?;
        if bytes[i] == b'<'
            && let Some(end) = find_byte(&bytes[i..], b'>', 7, budget)?
        {
            let tag = &text[i..i + end + 1];
            if tag.eq_ignore_ascii_case("<br>")
                || tag.eq_ignore_ascii_case("<br/>")
                || tag.eq_ignore_ascii_case("<br />")
            {
                push(&mut out, "\n", style, link, title, budget)?;
                i += end + 1;
                continue;
            }
        }
        if bytes[i] == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            push(&mut out, &text[i + 1..i + 2], style, link, title, budget)?;
            i += 2;
            continue;
        }
        if bytes[i] == b'&'
            && let Some(end) = find_byte(&bytes[i..], b';', 33, budget)?
            && let Some(decoded) = entity(&text[i + 1..i + end])
        {
            push(&mut out, &decoded, style, link, title, budget)?;
            i += end + 1;
            continue;
        }
        if bytes[i] == b'`' {
            let count = run_length(&bytes[i..], b'`', budget)?;
            let token = &text[i..i + count];
            let start = i + count;
            if let Some(end) = find_token(text, start, token, budget)? {
                let code = text[start..end].replace('\n', " ");
                let code =
                    if code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                        &code[1..code.len() - 1]
                    } else {
                        &code
                    };
                push(&mut out, code, style | CODE, link, title, budget)?;
                i = end + count;
                continue;
            }
            push(&mut out, token, style, link, title, budget)?;
            i += count;
            continue;
        }
        if matches!(bytes[i], b'*' | b'_' | b'~') {
            let ch = bytes[i];
            let available = scan(&bytes[i..], 3, budget, |candidate| candidate != ch)?
                .unwrap_or((bytes.len() - i).min(3));
            let count = if ch == b'~' {
                if available >= 2 { 2 } else { 0 }
            } else {
                available.min(3)
            };
            if count > 0 && opens(text, i, count, ch) {
                let token = &text[i..i + count];
                if let Some(end) = find_close(text, i + count, token, ch, budget)? {
                    let flags = match count {
                        1 => ITALIC,
                        2 if ch == b'~' => STRIKE,
                        2 => BOLD,
                        _ => BOLD | ITALIC,
                    };
                    let nested = parse(
                        &text[i + count..end],
                        references,
                        budget,
                        depth + 1,
                        style | flags,
                        link,
                        title,
                    )?;
                    for run in nested {
                        push(
                            &mut out,
                            &run.text,
                            run.style,
                            run.link.as_deref(),
                            run.title.as_deref(),
                            budget,
                        )?;
                    }
                    i = end + count;
                    continue;
                }
            }
        }
        if bytes[i] == b'['
            && link.is_none()
            && (i == 0 || bytes[i - 1] != b'!')
            && let Some(end) = label_end(text, i, budget)?
        {
            let label = &text[i + 1..end];
            let after = &text[end + 1..];
            let mut target = None;
            let mut consumed = 0;
            if let Some(rest) = after.strip_prefix('(') {
                // A failed destination can otherwise rescan the same long
                // suffix at each label. Charge before its linear parser runs.
                budget.charge(rest.len())?;
                if let Some((dest, title, length)) = destination(rest, true) {
                    target = Some((dest, title));
                    consumed = length + 1;
                }
            } else if let Some(rest) = after.strip_prefix('[') {
                if let Some(close) = find_byte(rest.as_bytes(), b']', rest.len(), budget)? {
                    let name = if close == 0 { label } else { &rest[..close] };
                    target = reference_target(references, name, budget)?;
                    consumed = close + 2;
                }
            } else {
                target = reference_target(references, label, budget)?;
            }
            if let Some((dest, tooltip)) = target {
                let approved = safe_link(&dest);
                let nested = parse(
                    label,
                    references,
                    budget,
                    depth + 1,
                    style,
                    approved.then_some(dest.as_str()),
                    tooltip.as_deref(),
                )?;
                for run in nested {
                    push(
                        &mut out,
                        &run.text,
                        run.style,
                        run.link.as_deref(),
                        run.title.as_deref(),
                        budget,
                    )?;
                }
                i = end + 1 + consumed;
                continue;
            }
        }
        if bytes[i] == b'<'
            && link.is_none()
            && let Some(end) = find_byte(&bytes[i + 1..], b'>', 4097, budget)?
        {
            let target = &text[i + 1..i + 1 + end];
            if safe_link(target) {
                push(&mut out, target, style, Some(target), None, budget)?;
                i += end + 2;
                continue;
            }
        }
        let ch = text[i..].chars().next().ok_or(Error::InvalidInput)?;
        let value = if ch == '\t' {
            " "
        } else {
            &text[i..i + ch.len_utf8()]
        };
        if value == " "
            && out.last().is_some_and(|run| {
                run.style == style && run.link.as_deref() == link && run.text.ends_with(' ')
            })
        {
            i += ch.len_utf8();
            continue;
        }
        push(&mut out, value, style, link, title, budget)?;
        i += ch.len_utf8();
    }
    Ok(out)
}
fn punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '“' | '”' | '‘' | '’' | '—' | '–' | '。' | '，' | '！' | '？' | '（' | '）'
        )
}
fn opens(text: &str, i: usize, n: usize, ch: u8) -> bool {
    let before = text[..i].chars().next_back();
    let after = text[i + n..].chars().next();
    let left = after.is_some_and(|c| {
        !c.is_whitespace()
            && (!punctuation(c) || before.is_none_or(|b| b.is_whitespace() || punctuation(b)))
    });
    left && (ch != b'_' || before.is_none_or(|b| !b.is_alphanumeric()))
}
fn closes(text: &str, i: usize, n: usize, ch: u8) -> bool {
    let before = text[..i].chars().next_back();
    let after = text[i + n..].chars().next();
    let right = before.is_some_and(|c| {
        !c.is_whitespace()
            && (!punctuation(c) || after.is_none_or(|b| b.is_whitespace() || punctuation(b)))
    });
    right && (ch != b'_' || after.is_none_or(|b| !b.is_alphanumeric()))
}
fn find_token(
    text: &str,
    mut i: usize,
    token: &str,
    budget: &mut Budget<'_>,
) -> Result<Option<usize>, Error> {
    while i < text.len() {
        budget.charge(1)?;
        if text.as_bytes()[i] == token.as_bytes()[0] {
            let count = run_length(&text.as_bytes()[i..], token.as_bytes()[0], budget)?;
            if count == token.len() {
                return Ok(Some(i));
            }
            i += count;
            continue;
        }
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    Ok(None)
}
fn find_close(
    text: &str,
    mut i: usize,
    token: &str,
    ch: u8,
    budget: &mut Budget<'_>,
) -> Result<Option<usize>, Error> {
    let mut inner = Vec::new();
    while i < text.len() {
        budget.charge(1)?;
        if text.as_bytes()[i] == b'\\' {
            i += 1;
            if i < text.len() {
                i += text[i..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if text.as_bytes()[i] == b'`' {
            let count = run_length(&text.as_bytes()[i..], b'`', budget)?;
            if let Some(end) = find_token(text, i + count, &text[i..i + count], budget)? {
                i = end + count;
                continue;
            }
            i += count;
            continue;
        }
        if text.as_bytes()[i] == ch {
            let count = run_length(&text.as_bytes()[i..], ch, budget)?;
            if closes(text, i, count, ch) {
                let mut consumed = 0;
                while inner.last().is_some_and(|n| *n <= count - consumed) {
                    consumed += inner.pop().unwrap();
                }
                if inner.is_empty() && count - consumed >= token.len() {
                    return Ok(Some(i + consumed));
                }
            }
            if opens(text, i, count, ch) {
                inner.push(count);
                if inner.len() > 32 {
                    return Err(Error::Limit);
                }
            }
            i += count;
            continue;
        }
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    Ok(None)
}
fn label_end(text: &str, start: usize, budget: &mut Budget<'_>) -> Result<Option<usize>, Error> {
    let mut nesting = 0;
    let mut escaped = false;
    for (i, ch) in text[start..].char_indices() {
        budget.charge(1)?;
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => {
                nesting += 1;
                if nesting > 32 {
                    return Err(Error::Limit);
                }
            }
            ']' => {
                nesting -= 1;
                if nesting == 0 {
                    return Ok(Some(start + i));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}
fn destination(text: &str, parenthesized: bool) -> Option<(String, Option<String>, usize)> {
    let trimmed = text.trim_start();
    let leading = text.len() - trimmed.len();
    let mut i = leading;
    let start = i;
    let destination = if text.as_bytes().get(i) == Some(&b'<') {
        i += 1;
        let offset = text[i..].find('>')?;
        let dest = &text[i..i + offset];
        if dest.contains(['<', '\n']) {
            return None;
        }
        i += offset + 1;
        unescape(dest)
    } else {
        let mut nesting = 0;
        let mut escape = false;
        for ch in text[i..].chars() {
            if escape {
                escape = false;
                i += ch.len_utf8();
                continue;
            }
            if ch == '\\' {
                escape = true;
                i += 1;
                continue;
            }
            if ch.is_whitespace() || (ch == ')' && nesting == 0) {
                break;
            }
            if ch == '(' {
                nesting += 1;
                if nesting > 32 {
                    return None;
                }
            }
            if ch == ')' {
                nesting -= 1;
            }
            i += ch.len_utf8();
        }
        if nesting != 0 {
            return None;
        }
        unescape(&text[start..i])
    };
    let mut title = None;
    let before_whitespace = i;
    while text.as_bytes().get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    if i > before_whitespace
        && text
            .as_bytes()
            .get(i)
            .is_some_and(|b| matches!(b, b'\'' | b'"'))
    {
        let quote = text.as_bytes()[i];
        i += 1;
        let title_start = i;
        while text.as_bytes().get(i).is_some_and(|b| *b != quote) {
            i += 1;
        }
        if text.as_bytes().get(i) != Some(&quote) {
            return None;
        }
        title = Some(unescape(&text[title_start..i]));
        i += 1;
        while text.as_bytes().get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
    }
    if parenthesized {
        if text.as_bytes().get(i) != Some(&b')') {
            return None;
        }
        i += 1;
    }
    Some((destination, title, i))
}
fn unescape(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            output.push(chars.next().unwrap());
        } else {
            output.push(ch);
        }
    }
    output
}
fn entity(text: &str) -> Option<String> {
    let value = if let Some(hex) = text.strip_prefix("#x").or_else(|| text.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .filter(|ch| *ch != '\0')
            .unwrap_or('\u{fffd}')
            .to_string()
    } else if let Some(decimal) = text.strip_prefix('#') {
        decimal
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .filter(|ch| *ch != '\0')
            .unwrap_or('\u{fffd}')
            .to_string()
    } else {
        match text {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{a0}",
            "copy" => "©",
            "reg" => "®",
            _ => return None,
        }
        .into()
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn search_windows_include_the_last_allowed_byte_but_never_the_suffix() {
        let cancel = AtomicBool::new(false);
        for limit in [7, 33, 4097] {
            let mut bytes = vec![b'x'; super::super::MAX_INPUT];
            bytes[limit] = b'>';
            let mut budget = Budget {
                cancel: &cancel,
                work: 0,
                runs: 0,
            };
            assert_eq!(find_byte(&bytes, b'>', limit, &mut budget), Ok(None));
            assert_eq!(budget.work, limit);
            bytes[limit - 1] = b'>';
            budget.work = 0;
            assert_eq!(
                find_byte(&bytes, b'>', limit, &mut budget),
                Ok(Some(limit - 1))
            );
            assert_eq!(budget.work, limit);
        }
        let mut budget = Budget {
            cancel: &cancel,
            work: 0,
            runs: 0,
        };
        assert_eq!(find_byte("🦀>".as_bytes(), b'>', 4, &mut budget), Ok(None));
        assert_eq!(
            find_byte("🦀>".as_bytes(), b'>', 5, &mut budget),
            Ok(Some(4))
        );
    }

    #[test]
    fn exhausted_work_is_rejected_before_even_an_immediate_match() {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            work: super::super::MAX_WORK - 63,
            runs: 0,
        };
        let bytes = vec![b'>'; super::super::MAX_INPUT];
        assert_eq!(
            find_byte(&bytes, b'>', 4097, &mut budget),
            Err(Error::Limit)
        );
        assert_eq!(budget.work, super::super::MAX_WORK + 1);
    }

    #[test]
    fn cancellation_after_search_progress_is_checked_before_the_next_chunk() {
        let cancel = AtomicBool::new(false);
        let bytes = vec![b'x'; super::super::MAX_INPUT];
        let mut budget = Budget {
            cancel: &cancel,
            work: 0,
            runs: 0,
        };
        assert_eq!(find_byte(&bytes, b'>', 64, &mut budget), Ok(None));
        assert_eq!(budget.work, 64);
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            find_byte(&bytes[64..], b'>', 4097, &mut budget),
            Err(Error::Cancelled)
        );
        assert_eq!(budget.work, 128);
        assert_eq!(run_length(&bytes, b'x', &mut budget), Err(Error::Cancelled));
        assert_eq!(budget.work, 192);
    }

    #[test]
    fn reference_metadata_reserves_work_before_cloning_or_emitting() {
        let cancel = AtomicBool::new(false);
        let mut references = References::new();
        references.insert(
            "note".into(),
            ("https://example.test/".into(), Some("t".repeat(4096))),
        );
        let mut budget = Budget {
            cancel: &cancel,
            work: super::super::MAX_WORK - 32,
            runs: 0,
        };
        assert_eq!(
            reference_target(&references, "note", &mut budget),
            Err(Error::Limit)
        );
        budget.work = super::super::MAX_WORK - 32;
        let mut output = Vec::new();
        assert_eq!(
            push(
                &mut output,
                "x",
                0,
                None,
                Some(&"t".repeat(4096)),
                &mut budget
            ),
            Err(Error::Limit)
        );
        assert!(output.is_empty());
        assert_eq!(budget.runs, 0);
    }
}
