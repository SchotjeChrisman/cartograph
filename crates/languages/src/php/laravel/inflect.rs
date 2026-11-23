//! English plurals and snake case, the way Laravel names tables and resource parameters.
//! A heuristic: irregular words outside [`IRREGULAR`] are not recognised.

const IRREGULAR: [(&str, &str); 7] = [
    ("person", "people"),
    ("child", "children"),
    ("man", "men"),
    ("woman", "women"),
    ("mouse", "mice"),
    ("foot", "feet"),
    ("tooth", "teeth"),
];

pub(super) fn plural(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if let Some((_, plural)) = IRREGULAR.iter().find(|(singular, _)| *singular == lower) {
        return plural.to_string();
    }
    if lower.ends_with(['s', 'x', 'z']) || lower.ends_with("ch") || lower.ends_with("sh") {
        return format!("{lower}es");
    }
    match lower.strip_suffix('y') {
        Some(stem) if !stem.ends_with(['a', 'e', 'i', 'o', 'u']) && !stem.is_empty() => {
            format!("{stem}ies")
        }
        _ => format!("{lower}s"),
    }
}

pub(super) fn singular(word: &str) -> String {
    if let Some((singular, _)) = IRREGULAR.iter().find(|(_, plural)| *plural == word) {
        return singular.to_string();
    }
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    for suffix in ["ses", "xes", "zes", "ches", "shes"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            return format!("{stem}{}", &suffix[..suffix.len() - 2]);
        }
    }
    word.strip_suffix('s').unwrap_or(word).to_string()
}

/// `UserProfile` becomes `user_profile`.
pub(super) fn snake_case(name: &str) -> String {
    let mut snake = String::with_capacity(name.len() + 4);
    for (position, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if position > 0 {
                snake.push('_');
            }
            snake.push(character.to_ascii_lowercase());
        } else {
            snake.push(character);
        }
    }
    snake
}

/// The table Eloquent uses for a model class named `class_name`.
pub(super) fn conventional_table(class_name: &str) -> String {
    let snake = snake_case(class_name);
    match snake.rsplit_once('_') {
        Some((head, last)) => format!("{head}_{}", plural(last)),
        None => plural(&snake),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_follow_laravel_conventions() {
        assert_eq!(conventional_table("User"), "users");
        assert_eq!(conventional_table("UserProfile"), "user_profiles");
        assert_eq!(conventional_table("Category"), "categories");
        assert_eq!(conventional_table("Box"), "boxes");
        assert_eq!(conventional_table("Day"), "days");
        assert_eq!(conventional_table("Person"), "people");
        assert_eq!(conventional_table("Batch"), "batches");
    }

    #[test]
    fn singular_undoes_plural_for_resource_parameters() {
        assert_eq!(singular("photos"), "photo");
        assert_eq!(singular("categories"), "category");
        assert_eq!(singular("boxes"), "box");
        assert_eq!(singular("people"), "person");
    }
}
