use super::{BOLD, Budget, CODE, Error, ITALIC, MAX_RUNS, Run, STRIKE};
use std::collections::BTreeMap;
type References = BTreeMap<String, (String, Option<String>)>;
pub(super) fn reference(line: &str) -> Option<(String, String, Option<String>)> {
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
fn key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
    budget.charge(text.len())?;
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
            && let Some(end) = text[i..].find('>').filter(|n| *n <= 6)
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
            && let Some(end) = text[i..].find(';').filter(|n| *n <= 32)
            && let Some(decoded) = entity(&text[i + 1..i + end])
        {
            push(&mut out, &decoded, style, link, title, budget)?;
            i += end + 1;
            continue;
        }
        if bytes[i] == b'`' {
            let count = bytes[i..].iter().take_while(|b| **b == b'`').count();
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
            let available = bytes[i..].iter().take_while(|b| **b == ch).count();
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
                if let Some((dest, title, length)) = destination(rest, true) {
                    target = Some((dest, title));
                    consumed = length + 1;
                }
            } else if let Some(rest) = after.strip_prefix('[') {
                if let Some(close) = rest.find(']') {
                    let name = if close == 0 { label } else { &rest[..close] };
                    target = references.get(&key(name)).cloned();
                    consumed = close + 2;
                }
            } else {
                target = references.get(&key(label)).cloned();
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
            && let Some(end) = text[i + 1..].find('>').filter(|n| *n <= 4096)
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
            let count = text.as_bytes()[i..]
                .iter()
                .take_while(|b| **b == token.as_bytes()[0])
                .count();
            budget.charge(count)?;
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
            let count = text.as_bytes()[i..]
                .iter()
                .take_while(|b| **b == b'`')
                .count();
            if let Some(end) = find_token(text, i + count, &text[i..i + count], budget)? {
                i = end + count;
                continue;
            }
            i += count;
            continue;
        }
        if text.as_bytes()[i] == ch {
            let count = text.as_bytes()[i..]
                .iter()
                .take_while(|b| **b == ch)
                .count();
            budget.charge(count)?;
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
