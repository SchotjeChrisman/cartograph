//! The Laravel plugin, layered on the PHP module: it reads framework wiring out of the same
//! syntax tree and adds routes, Eloquent tables, container bindings, events and listeners,
//! config keys and Blade views to the graph. It runs only for projects whose `composer.json`
//! requires `laravel/framework`.

mod arguments;
mod blade;
mod config;
mod container;
mod eloquent;
mod events;
mod inflect;
pub(super) mod resolve;
mod routes;
mod views;

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::{FrameworkLink, Reference, ReferenceKind};

use super::extract::Extractor;

/// State the plugin keeps while the extractor walks a file.
#[derive(Default)]
pub(super) struct Plugin {
    route_groups: Vec<routes::RouteGroup>,
}

pub(super) fn visit(extractor: &mut Extractor<'_>, capture: &str, node: Node<'_>) {
    let Some(plugin) = extractor.laravel.as_mut() else {
        return;
    };
    plugin
        .route_groups
        .retain(|group| group.end_byte > node.start_byte());
    match capture {
        "class" => eloquent::model_class(extractor, node),
        "property" => {
            eloquent::table_property(extractor, node);
            events::listen_property(extractor, node);
        }
        "method" => events::handler_method(extractor, node),
        "call" => {
            events::event_helper(extractor, node);
            config::config_helper(extractor, node);
            views::view_helper(extractor, node);
        }
        "scoped_call" => {
            routes::visit_call(extractor, node);
            events::facade_call(extractor, node);
            config::facade_call(extractor, node);
            eloquent::database_call(extractor, node);
            container::facade_binding(extractor, node);
            views::facade_call(extractor, node);
        }
        "member_call" => {
            routes::visit_call(extractor, node);
            eloquent::database_call(extractor, node);
            container::member_binding(extractor, node);
        }
        "return" => config::config_file(extractor, node),
        _ => {}
    }
}

/// Runs after the walk, for patterns that read the whole file as text.
pub(super) fn finish(extractor: &mut Extractor<'_>) {
    if extractor.laravel.is_some() {
        blade::views_of_template(extractor);
        drop_reads_covered_by_writes(extractor);
    }
}

/// `DB::table('x')->insert()` is seen as a read of `x` (the `table` call) and a write of `x`
/// (the chained verb) at the same spot; only the write is kept.
fn drop_reads_covered_by_writes(extractor: &mut Extractor<'_>) {
    let access = |reference: &Reference| match reference.kind {
        ReferenceKind::Framework(FrameworkLink::TableAccess { write }) => Some(write),
        _ => None,
    };
    let writes: HashSet<(Option<usize>, String, u32)> = extractor
        .extraction
        .references
        .iter()
        .filter(|reference| access(reference) == Some(true))
        .map(|reference| (reference.from, reference.text.clone(), reference.line))
        .collect();
    extractor.extraction.references.retain(|reference| {
        access(reference) != Some(false)
            || !writes.contains(&(reference.from, reference.text.clone(), reference.line))
    });
}
