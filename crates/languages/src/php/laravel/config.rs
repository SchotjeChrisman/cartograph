//! Config keys: `config('app.name')` reads them and `config/*.php` files define them.

use tree_sitter::Node;

use super::arguments::{arguments, array_elements, short_name, string_literal};
use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;

const CONFIG_HELPER: &str = "config";
const CONFIG_FACADE: &str = "Config";
const READING_METHODS: [&str; 6] = ["get", "string", "integer", "boolean", "array", "has"];
const WRITING_METHODS: [&str; 1] = ["set"];
const CONFIG_DIRECTORY: &str = "config/";

/// `config('app.name')`
pub(super) fn config_helper(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let is_helper = node
        .child_by_field_name("function")
        .is_some_and(|callee| extractor.text(callee) == CONFIG_HELPER);
    if is_helper
        && let Some(key) = arguments(node)
            .first()
            .and_then(|argument| string_literal(extractor, *argument))
    {
        push_access(extractor, node, key, false);
    }
}

/// `Config::get('app.name')` and `Config::set('app.name', ...)`
pub(super) fn facade_call(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let (Some(scope), Some(name)) = (
        node.child_by_field_name("scope"),
        node.child_by_field_name("name"),
    ) else {
        return;
    };
    if short_name(&extractor.text(scope)) != CONFIG_FACADE {
        return;
    }
    let method = extractor.text(name);
    let write = WRITING_METHODS.contains(&method.as_str());
    if !write && !READING_METHODS.contains(&method.as_str()) {
        return;
    }
    if let Some(key) = arguments(node)
        .first()
        .and_then(|argument| string_literal(extractor, *argument))
    {
        push_access(extractor, node, key, write);
    }
}

/// The array a `config/name.php` file returns defines `name.key` and each nested key below it.
pub(super) fn config_file(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let Some(stem) = config_file_stem(extractor.path) else {
        return;
    };
    let at_top_level = node
        .parent()
        .is_some_and(|parent| parent.kind() == "program");
    let returned = node.named_child(0);
    if let (true, Some(returned)) = (at_top_level, returned) {
        define_keys(extractor, returned, &stem);
    }
}

fn config_file_stem(path: &str) -> Option<String> {
    let name = path.strip_prefix(CONFIG_DIRECTORY).or_else(|| {
        path.split_once(&format!("/{CONFIG_DIRECTORY}"))
            .map(|(_, rest)| rest)
    })?;
    let stem = name.strip_suffix(".php")?;
    (!stem.contains('/')).then(|| stem.to_string())
}

fn define_keys(extractor: &mut Extractor<'_>, array: Node<'_>, prefix: &str) {
    for (key, value) in array_elements(array) {
        let Some(key) = key.and_then(|key| string_literal(extractor, key)) else {
            continue;
        };
        let full = format!("{prefix}.{key}");
        let mut reference = extractor.reference(
            ReferenceKind::Framework(FrameworkLink::ConfigAccess { write: true }),
            full.clone(),
            value,
        );
        reference.from = None;
        extractor.push_reference(reference);
        define_keys(extractor, value, &full);
    }
}

fn push_access(extractor: &mut Extractor<'_>, node: Node<'_>, key: String, write: bool) {
    let reference = extractor.reference(
        ReferenceKind::Framework(FrameworkLink::ConfigAccess { write }),
        key,
        node,
    );
    extractor.push_reference(reference);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_directly_in_a_config_directory_define_keys() {
        assert_eq!(config_file_stem("config/app.php").as_deref(), Some("app"));
        assert_eq!(
            config_file_stem("shop/config/mail.php").as_deref(),
            Some("mail")
        );
        assert_eq!(config_file_stem("config/nested/x.php"), None);
        assert_eq!(config_file_stem("app/config.php"), None);
    }
}
