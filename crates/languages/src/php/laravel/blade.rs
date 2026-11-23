//! Blade templates: `@include`, `@extends` and `<x-component>` tags name other views. Blade is
//! text with directives, so it is read as text rather than through the PHP grammar.

use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;

const TEMPLATE_SUFFIX: &str = ".blade.php";
const DIRECTIVES: [&str; 5] = [
    "@include(",
    "@includeIf(",
    "@extends(",
    "@each(",
    "@component(",
];
const COMPONENT_TAG: &str = "<x-";
const COMPONENT_VIEW_PREFIX: &str = "components.";
/// `<x-slot>` fills a named slot; it is not a component file.
const SLOT_COMPONENT: &str = "slot";

pub(super) fn views_of_template(extractor: &mut Extractor<'_>) {
    if !extractor.path.ends_with(TEMPLATE_SUFFIX) {
        return;
    }
    let text = String::from_utf8_lossy(extractor.source).into_owned();
    for (index, line) in text.lines().enumerate() {
        for view in views_on_line(line) {
            let mut reference = extractor.reference_at_line(
                ReferenceKind::Framework(FrameworkLink::View),
                view,
                index as u32 + 1,
            );
            reference.from = None;
            extractor.push_reference(reference);
        }
    }
}

/// The view names a line of a template refers to.
fn views_on_line(line: &str) -> Vec<String> {
    let directives = DIRECTIVES.iter().flat_map(|directive| {
        line.match_indices(directive)
            .filter_map(|(at, _)| quoted_after(&line[at + directive.len()..]))
    });
    let components = line
        .match_indices(COMPONENT_TAG)
        .filter_map(|(at, _)| component_view(&line[at + COMPONENT_TAG.len()..]));
    directives.chain(components).collect()
}

/// The string literal at the start of `text`, such as `'layouts.app'` in `'layouts.app')`.
fn quoted_after(text: &str) -> Option<String> {
    let quote = text
        .chars()
        .next()
        .filter(|character| matches!(character, '\'' | '"'))?;
    let inner = &text[1..];
    inner.find(quote).map(|end| inner[..end].to_string())
}

/// `alert` in `<x-alert type="x">` is the view `components.alert`.
fn component_view(after_tag: &str) -> Option<String> {
    let name: String = after_tag
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
        .collect();
    (!name.is_empty() && name != SLOT_COMPONENT).then(|| format!("{COMPONENT_VIEW_PREFIX}{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_directives_and_component_tags() {
        assert_eq!(views_on_line("@extends('layouts.app')"), ["layouts.app"]);
        assert_eq!(
            views_on_line("  @include(\"partials.nav\", ['a' => 1])"),
            ["partials.nav"]
        );
        assert_eq!(
            views_on_line("<x-forms.input name=\"a\" /> <x-slot name=\"b\">"),
            ["components.forms.input"]
        );
        assert!(views_on_line("@if($a) {{ $b }} @endif").is_empty());
    }
}
