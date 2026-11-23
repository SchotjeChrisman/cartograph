/// `current` with `body` between the `begin` and `end` marker lines, replacing an earlier marked
/// section in place or appending a new one.
pub fn with_marked_section(current: &str, begin: &str, end: &str, body: &str) -> String {
    let section = format!("{begin}\n{body}\n{end}\n");
    match (current.find(begin), current.find(end)) {
        (Some(start), Some(stop)) if start < stop => {
            let after = &current[stop + end.len()..];
            format!(
                "{}{section}{}",
                &current[..start],
                after.trim_start_matches('\n')
            )
        }
        _ if current.trim().is_empty() => section,
        _ => format!("{}\n\n{section}", current.trim_end()),
    }
}
