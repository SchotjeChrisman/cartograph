//! One level of the map: the children of a node as boxes, and the arrows between them.

use std::collections::HashMap;

use describe::Overrides;
use graph::{Component, ComponentId, Freshness, Query, Reader};
use serde::Serialize;

use crate::arrows::{self, Arrow, Candidate, MAX_ARROWS, MAX_BOXES};
use crate::error::{Error, Result};
use crate::labels::labels;
use crate::node_id::NodeId;

#[derive(Debug, Serialize)]
pub struct BoxView {
    pub id: NodeId,
    pub kind: &'static str,
    pub name: String,
    pub summary: Option<String>,
    pub stale: bool,
    pub file_count: usize,
}

#[derive(Debug, Serialize)]
pub struct Crumb {
    pub id: NodeId,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct LevelView {
    pub breadcrumb: Vec<Crumb>,
    pub boxes: Vec<BoxView>,
    pub arrows: Vec<Arrow>,
    pub hidden_boxes: usize,
    pub freshness: Freshness,
}

/// The level whose boxes are the children of `parent`; `None` is the top level. A component
/// without child components shows its files.
pub fn level(reader: &Reader, overrides: &Overrides, parent: Option<NodeId>) -> Result<LevelView> {
    let parent_component = match parent {
        None => None,
        Some(NodeId::Component(id)) => Some(
            reader
                .component(id)?
                .ok_or_else(|| Error::NotFound(format!("component {}", id.0)))?,
        ),
        Some(NodeId::File(_)) => {
            return Err(Error::BadRequest("a file has no children".to_string()));
        }
    };
    let children = reader.components(parent_component.as_ref().map(|component| component.id))?;
    let files = reader.files()?;
    let (candidates, mut views, owner_of_file) = if children.is_empty() {
        match &parent_component {
            Some(parent) => file_boxes(&files, parent.id),
            None => (Vec::new(), HashMap::new(), HashMap::new()),
        }
    } else {
        component_boxes(reader, overrides, &children, &files)?
    };
    let arrows = arrows::aggregate(&reader.file_links()?, &owner_of_file);
    let visible = arrows::select_visible(candidates, arrows, MAX_BOXES, MAX_ARROWS);
    Ok(LevelView {
        breadcrumb: breadcrumb(reader, overrides, parent_component)?,
        boxes: visible
            .boxes
            .iter()
            .filter_map(|id| views.remove(id))
            .collect(),
        arrows: visible.arrows,
        hidden_boxes: visible.hidden_boxes,
        freshness: reader.freshness()?,
    })
}

type Boxes = (
    Vec<Candidate>,
    HashMap<NodeId, BoxView>,
    HashMap<i64, NodeId>,
);

fn file_boxes(files: &[graph::FileRow], parent: ComponentId) -> Boxes {
    let mut candidates = Vec::new();
    let mut views = HashMap::new();
    let mut owner_of_file = HashMap::new();
    for file in files.iter().filter(|file| file.component == Some(parent)) {
        let id = NodeId::File(file.id);
        candidates.push(Candidate {
            id,
            name: file.path.clone(),
        });
        views.insert(
            id,
            BoxView {
                id,
                kind: "file",
                name: file.path.clone(),
                summary: None,
                stale: false,
                file_count: 1,
            },
        );
        owner_of_file.insert(file.id, id);
    }
    (candidates, views, owner_of_file)
}

fn component_boxes(
    reader: &Reader,
    overrides: &Overrides,
    children: &[Component],
    files: &[graph::FileRow],
) -> Result<Boxes> {
    let mut box_of_component: HashMap<ComponentId, NodeId> = HashMap::new();
    let mut candidates = Vec::new();
    let mut views = HashMap::new();
    for child in children {
        let id = NodeId::Component(child.id);
        let shown = labels(reader, overrides, child)?;
        for member in subtree(reader, child.id)? {
            box_of_component.insert(member, id);
        }
        candidates.push(Candidate {
            id,
            name: shown.name.clone(),
        });
        views.insert(
            id,
            BoxView {
                id,
                kind: "component",
                stale: shown.is_stale(child),
                name: shown.name,
                summary: shown.summary,
                file_count: 0,
            },
        );
    }
    let mut owner_of_file = HashMap::new();
    for file in files {
        let Some(&id) = file.component.and_then(|leaf| box_of_component.get(&leaf)) else {
            continue;
        };
        owner_of_file.insert(file.id, id);
        if let Some(view) = views.get_mut(&id) {
            view.file_count += 1;
        }
    }
    Ok((candidates, views, owner_of_file))
}

/// `root` and every component below it.
fn subtree(reader: &Reader, root: ComponentId) -> Result<Vec<ComponentId>> {
    let mut members = vec![root];
    let mut next = 0;
    while let Some(&current) = members.get(next) {
        next += 1;
        members.extend(
            reader
                .components(Some(current))?
                .into_iter()
                .map(|child| child.id),
        );
    }
    Ok(members)
}

/// From the top level down to `component`, which is last.
fn breadcrumb(
    reader: &Reader,
    overrides: &Overrides,
    component: Option<Component>,
) -> Result<Vec<Crumb>> {
    let mut crumbs = Vec::new();
    let mut current = component;
    while let Some(component) = current {
        crumbs.push(Crumb {
            id: NodeId::Component(component.id),
            name: labels(reader, overrides, &component)?.name,
        });
        current = match component.parent {
            Some(parent) => reader.component(parent)?,
            None => None,
        };
    }
    crumbs.reverse();
    Ok(crumbs)
}
