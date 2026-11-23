//! A reader for the small subset of YAML `links.yaml` uses: a `links:` key holding a list of
//! flat entries with scalar values. Anything else is an error that names its line, so a typo
//! fails loudly instead of dropping a link.

use super::Link;

const KEYS: [&str; 4] = ["from", "to", "kind", "note"];
const LIST_KEY: &str = "links";
/// Characters that start YAML syntax a plain scalar cannot begin with.
const RESERVED_STARTS: [char; 13] = [
    '[', ']', '{', '}', '|', '>', '&', '*', '!', '%', '@', '`', ',',
];

#[derive(Default)]
struct Entry {
    line: u32,
    values: Vec<(&'static str, String)>,
}

pub fn parse(text: &str) -> Result<Vec<Link>, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index as u32 + 1, strip_comment(line)))
        .filter(|(_, line)| !line.trim().is_empty());
    match lines.next() {
        Some((_, header)) if header.trim_end() == format!("{LIST_KEY}:") => {}
        Some((number, other)) => {
            return Err(format!(
                "line {number}: expected `{LIST_KEY}:` first, found `{}`",
                other.trim()
            ));
        }
        None => return Ok(Vec::new()),
    }
    let mut entries: Vec<Entry> = Vec::new();
    for (number, line) in lines {
        let trimmed = line.trim();
        let (starts_entry, pair) = match trimmed.strip_prefix("- ") {
            Some(rest) => (true, rest.trim_start()),
            None => (false, trimmed),
        };
        if starts_entry {
            entries.push(Entry {
                line: number,
                values: Vec::new(),
            });
        }
        let entry = entries
            .last_mut()
            .ok_or_else(|| format!("line {number}: expected `- ` to start an entry"))?;
        let (key, value) = read_pair(pair, number)?;
        if entry.values.iter().any(|(existing, _)| *existing == key) {
            return Err(format!("line {number}: `{key}` appears twice in one entry"));
        }
        entry.values.push((key, value));
    }
    entries.into_iter().map(into_link).collect()
}

fn into_link(entry: Entry) -> Result<Link, String> {
    let take = |wanted: &str| {
        entry
            .values
            .iter()
            .find(|(key, _)| *key == wanted)
            .map(|(_, value)| value.clone())
    };
    let require = |wanted: &str| {
        take(wanted).ok_or_else(|| format!("line {}: entry has no `{wanted}`", entry.line))
    };
    Ok(Link {
        from: take("from"),
        to: require("to")?,
        kind: require("kind")?,
        note: take("note"),
        line: entry.line,
    })
}

fn read_pair(pair: &str, number: u32) -> Result<(&'static str, String), String> {
    let (key, value) = pair
        .split_once(':')
        .ok_or_else(|| format!("line {number}: expected `key: value`, found `{pair}`"))?;
    let key = key.trim();
    let key = KEYS.iter().find(|known| **known == key).ok_or_else(|| {
        format!(
            "line {number}: unknown key `{key}`; use one of {}",
            KEYS.join(", ")
        )
    })?;
    Ok((key, scalar(value.trim(), number)?))
}

fn scalar(value: &str, number: u32) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("line {number}: empty value"));
    }
    let Some(quote) = value
        .chars()
        .next()
        .filter(|first| matches!(first, '"' | '\''))
    else {
        return plain_scalar(value, number);
    };
    let inner = value[1..]
        .strip_suffix(quote)
        .ok_or_else(|| format!("line {number}: quoted value is not closed: {value}"))?;
    Ok(if quote == '"' {
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        inner.replace("''", "'")
    })
}

fn plain_scalar(value: &str, number: u32) -> Result<String, String> {
    if value.starts_with(RESERVED_STARTS) || value.contains(": ") {
        return Err(format!("line {number}: put `{value}` in quotes"));
    }
    Ok(value.to_string())
}

/// The line without a trailing ` # comment`; a `#` inside quotes stays.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous_is_space = true;
    for (position, character) in line.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '#') if previous_is_space => return line[..position].trim_end(),
            _ => {}
        }
        previous_is_space = character.is_whitespace();
    }
    line.trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_quotes_and_comments() {
        let text = "# header\nlinks:\n  - from: App\\Jobs\\Send::handle  # runs later\n    to: \"App\\Mail\"\n    kind: queue\n    note: 'it''s nightly: 02:00'\n  - to: bin/prune.php\n    kind: cron\n";
        let links = parse(text).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].from.as_deref(), Some("App\\Jobs\\Send::handle"));
        assert_eq!(links[0].to, "App\\Mail");
        assert_eq!(links[0].note.as_deref(), Some("it's nightly: 02:00"));
        assert_eq!((links[0].line, links[1].line), (3, 7));
        assert_eq!(links[1].from, None);
    }

    #[test]
    fn rejects_what_it_does_not_understand_with_a_line_number() {
        assert!(
            parse("links:\n  - to: a\n    kind: x\n    colour: red\n")
                .unwrap_err()
                .contains("line 4: unknown key `colour`")
        );
        assert!(
            parse("links:\n  - to: a\n")
                .unwrap_err()
                .contains("no `kind`")
        );
        assert!(
            parse("links:\n  - to: [a]\n    kind: x\n")
                .unwrap_err()
                .contains("line 2: put `[a]` in quotes")
        );
        assert!(
            parse("other:\n")
                .unwrap_err()
                .contains("expected `links:` first")
        );
        assert!(
            parse("links:\n  - to: \"a\n    kind: x\n")
                .unwrap_err()
                .contains("not closed")
        );
    }

    #[test]
    fn an_empty_file_declares_nothing() {
        assert_eq!(parse("# nothing yet\n").unwrap(), Vec::new());
    }
}
