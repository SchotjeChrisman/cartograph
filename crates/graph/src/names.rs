//! Splits identifiers into lowercase words so fuzzy search matches `getUserName` to `user`.

/// Words of an identifier or qualified name: separators split, and so do lower-to-upper
/// transitions and the end of an acronym (`HTMLParser` gives `html`, `parser`).
pub fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if !character.is_alphanumeric() {
            flush(&mut words, &mut current);
            previous = None;
            continue;
        }
        let starts_after_lower = character.is_uppercase()
            && previous.is_some_and(|before| before.is_lowercase() || before.is_ascii_digit());
        let ends_acronym = character.is_uppercase()
            && previous.is_some_and(char::is_uppercase)
            && characters.peek().is_some_and(|next| next.is_lowercase());
        if starts_after_lower || ends_acronym {
            flush(&mut words, &mut current);
        }
        current.extend(character.to_lowercase());
        previous = Some(character);
    }
    flush(&mut words, &mut current);
    words
}

fn flush(words: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::split_words;

    #[test]
    fn splits_separators_and_case() {
        assert_eq!(split_words("nsFoo::Init"), ["ns", "foo", "init"]);
        assert_eq!(split_words("get_user_name"), ["get", "user", "name"]);
        assert_eq!(split_words("HTMLParser"), ["html", "parser"]);
        assert_eq!(split_words("Vec3"), ["vec3"]);
        assert!(split_words("::").is_empty());
    }
}
