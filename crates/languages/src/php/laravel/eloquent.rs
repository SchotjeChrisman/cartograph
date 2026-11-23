//! Eloquent models and the tables they and the query builder touch.

use tree_sitter::Node;

use super::arguments::{arguments, short_name, string_literal};
use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;
use crate::syntax::{named_child_of_kind, named_children};

const TABLE_PROPERTY: &str = "$table";
const DATABASE_FACADE: &str = "DB";
const SCHEMA_FACADE: &str = "Schema";
const SCHEMA_METHODS: [&str; 5] = ["create", "table", "drop", "dropIfExists", "rename"];
const WRITE_METHODS: [&str; 6] = [
    "insert",
    "insertGetId",
    "update",
    "upsert",
    "delete",
    "truncate",
];

/// Any class that extends something may be a model; the resolver checks the hierarchy.
pub(super) fn model_class(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let Some(class) = extractor.enclosing_class() else {
        return;
    };
    if named_child_of_kind(node, "base_clause").is_some() {
        push_model_table(extractor, node, class, String::new());
    }
}

/// `protected $table = 'people';`
pub(super) fn table_property(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let Some(class) = extractor.enclosing_class() else {
        return;
    };
    for element in named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "property_element")
    {
        let named_table = element
            .child_by_field_name("name")
            .is_some_and(|name| extractor.text(name) == TABLE_PROPERTY);
        let table = element
            .child_by_field_name("default_value")
            .and_then(|value| string_literal(extractor, value));
        if let (true, Some(table)) = (named_table, table) {
            push_model_table(extractor, node, class, table);
        }
    }
}

fn push_model_table(extractor: &mut Extractor<'_>, node: Node<'_>, class: usize, table: String) {
    let mut reference = extractor.reference(
        ReferenceKind::Framework(FrameworkLink::ModelTable),
        table,
        node,
    );
    reference.from = Some(class);
    extractor.push_reference(reference);
}

/// `DB::table('users')`, `Schema::create('users', ...)` and writes chained onto `DB::table`.
pub(super) fn database_call(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let (Some(name), Some(owner)) = (node.child_by_field_name("name"), call_owner(node)) else {
        return;
    };
    let method = extractor.text(name);
    match (
        node.kind(),
        short_name(&extractor.text(owner)),
        method.as_str(),
    ) {
        ("scoped_call_expression", DATABASE_FACADE, "table") => {
            push_table_access(extractor, node, false)
        }
        ("scoped_call_expression", SCHEMA_FACADE, schema) if SCHEMA_METHODS.contains(&schema) => {
            push_table_access(extractor, node, true)
        }
        ("member_call_expression", DATABASE_FACADE, write) if WRITE_METHODS.contains(&write) => {
            push_table_access(extractor, node, true)
        }
        _ => {}
    }
}

/// The facade a call chain starts from, for `DB::table('x')->insert(...)` the `DB`.
fn call_owner(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node;
    while current.kind() == "member_call_expression" {
        current = current.child_by_field_name("object")?;
    }
    current.child_by_field_name("scope")
}

/// The table is the first argument of the `table` call at the start of the chain.
fn push_table_access(extractor: &mut Extractor<'_>, node: Node<'_>, write: bool) {
    let mut start = node;
    while let Some(object) = start.child_by_field_name("object") {
        start = object;
    }
    let table = arguments(start)
        .first()
        .and_then(|argument| string_literal(extractor, *argument));
    if let Some(table) = table {
        let reference = extractor.reference(
            ReferenceKind::Framework(FrameworkLink::TableAccess { write }),
            table,
            node,
        );
        extractor.push_reference(reference);
    }
}
