//! Where the prompt's graph facts come from. The trait is the seam; [`GraphSliceSource`] reads
//! them from the graph file.

use std::collections::HashMap;

use graph::{
    Component, ComponentId, DEPENDENCY_KINDS, Direction, FileLink, NodeInfo, Query, Reader,
};

use crate::error::Result;
use crate::prompt::{ComponentLink, ComponentSlice, SymbolLine};

const SYMBOL_LIMIT: usize = 40;
const ENTRY_POINT_LIMIT: usize = 10;
const CALLERS_CHECKED_PER_SYMBOL: usize = 20;
const LINK_LIMIT: usize = 10;

/// Supplies the graph part of a component's slice. `children` of the result is left empty; the
/// describer fills it from stored descriptions.
pub trait SliceSource {
    fn slice(&self, component: &Component) -> Result<ComponentSlice>;
}

/// Reads slices from the graph. The component tree, file assignments and file-to-file links are
/// loaded once up front, so one describe run costs one pass over them, not one per component.
pub struct GraphSliceSource<'a> {
    reader: &'a Reader,
    parent_of: HashMap<ComponentId, Option<ComponentId>>,
    name_of: HashMap<ComponentId, String>,
    file_component: HashMap<i64, ComponentId>,
    file_counts: HashMap<ComponentId, usize>,
    links: Vec<FileLink>,
}

impl<'a> GraphSliceSource<'a> {
    pub fn load(reader: &'a Reader) -> Result<Self> {
        let mut parent_of = HashMap::new();
        let mut name_of = HashMap::new();
        let mut pending = reader.components(None)?;
        while let Some(component) = pending.pop() {
            pending.extend(reader.components(Some(component.id))?);
            parent_of.insert(component.id, component.parent);
            name_of.insert(component.id, component.name);
        }
        let mut file_component = HashMap::new();
        let mut file_counts: HashMap<ComponentId, usize> = HashMap::new();
        for file in reader.files()? {
            let Some(leaf) = file.component else { continue };
            file_component.insert(file.id, leaf);
            for id in ancestors_and_self(&parent_of, leaf) {
                *file_counts.entry(id).or_default() += 1;
            }
        }
        Ok(GraphSliceSource {
            reader,
            parent_of,
            name_of,
            file_component,
            file_counts,
            links: reader.file_links()?,
        })
    }

    fn symbol_line(&self, symbol: &NodeInfo) -> Result<SymbolLine> {
        let (signature, doc) = self.reader.symbol_text(symbol.id)?;
        Ok(SymbolLine {
            kind: symbol.kind.to_string(),
            qualified_name: symbol.qualified_name.clone(),
            file: symbol.file.clone(),
            line: symbol.line,
            signature,
            doc,
        })
    }

    fn is_within(&self, inner: ComponentId, outer: ComponentId) -> bool {
        ancestors_and_self(&self.parent_of, inner).any(|id| id == outer)
    }

    /// Symbols some other component depends on, among the most connected ones.
    fn entry_points(
        &self,
        component: ComponentId,
        symbols: &[NodeInfo],
    ) -> Result<Vec<SymbolLine>> {
        let mut entries = Vec::new();
        for symbol in symbols {
            if entries.len() == ENTRY_POINT_LIMIT {
                break;
            }
            let callers = self.reader.neighbours(
                symbol.id,
                Direction::In,
                DEPENDENCY_KINDS,
                CALLERS_CHECKED_PER_SYMBOL,
            )?;
            let called_from_outside = callers.items.iter().any(|caller| {
                caller
                    .node
                    .component
                    .is_some_and(|other| !self.is_within(other, component))
            });
            if called_from_outside {
                entries.push(self.symbol_line(symbol)?);
            }
        }
        Ok(entries)
    }

    /// Edge counts to and from each sibling, strongest first.
    // ponytail: scans every file link per component; index links by file if describing a whole
    // Gecko-sized tree makes this the bottleneck.
    fn sibling_links(&self, component: ComponentId) -> Vec<ComponentLink> {
        let parent = self.parent_of.get(&component).copied().flatten();
        let sibling_of = |file: i64| -> Option<ComponentId> {
            let leaf = *self.file_component.get(&file)?;
            ancestors_and_self(&self.parent_of, leaf)
                .find(|id| self.parent_of.get(id).copied().flatten() == parent)
        };
        let mut totals: HashMap<ComponentId, (u64, u64)> = HashMap::new();
        for link in &self.links {
            let (Some(from), Some(to)) = (sibling_of(link.from_file), sibling_of(link.to_file))
            else {
                continue;
            };
            if from == component && to != component {
                totals.entry(to).or_default().0 += link.edge_count;
            } else if to == component && from != component {
                totals.entry(from).or_default().1 += link.edge_count;
            }
        }
        let mut links: Vec<ComponentLink> = totals
            .into_iter()
            .map(|(other, (outgoing, incoming))| ComponentLink {
                name: self.name_of[&other].clone(),
                outgoing,
                incoming,
            })
            .collect();
        links.sort_by(|a, b| {
            (b.outgoing + b.incoming)
                .cmp(&(a.outgoing + a.incoming))
                .then_with(|| a.name.cmp(&b.name))
        });
        links.truncate(LINK_LIMIT);
        links
    }
}

impl SliceSource for GraphSliceSource<'_> {
    fn slice(&self, component: &Component) -> Result<ComponentSlice> {
        let symbols = self.reader.top_symbols(component.id, SYMBOL_LIMIT)?;
        Ok(ComponentSlice {
            name: component.name.clone(),
            level: component.level,
            file_count: self.file_counts.get(&component.id).copied().unwrap_or(0),
            entry_points: self.entry_points(component.id, &symbols)?,
            symbols: symbols
                .iter()
                .map(|symbol| self.symbol_line(symbol))
                .collect::<Result<_>>()?,
            links: self.sibling_links(component.id),
            children: Vec::new(),
        })
    }
}

fn ancestors_and_self(
    parent_of: &HashMap<ComponentId, Option<ComponentId>>,
    start: ComponentId,
) -> impl Iterator<Item = ComponentId> + '_ {
    std::iter::successors(Some(start), |id| parent_of.get(id).copied().flatten())
}
