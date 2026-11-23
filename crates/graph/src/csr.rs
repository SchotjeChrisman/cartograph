//! A compressed-sparse-row slice of the graph, built per query so deep traversals run in memory
//! instead of as one SQL round trip per hop.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;

use crate::id::SymbolId;
use crate::kinds::{EdgeKind, Provenance};

/// Which way to follow edges, relative to the node a query starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// From source to target: what this node uses.
    Out,
    /// From target to source: what uses this node.
    In,
    Both,
}

impl Direction {
    pub(crate) fn directions(self) -> &'static [Direction] {
        match self {
            Direction::Out => &[Direction::Out],
            Direction::In => &[Direction::In],
            Direction::Both => &[Direction::Out, Direction::In],
        }
    }
}

/// One edge as loaded into a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceEdge {
    pub src: SymbolId,
    pub dst: SymbolId,
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub line: Option<u32>,
}

/// An edge seen from one of its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// Local index of the node at the other end.
    pub other: u32,
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub line: Option<u32>,
}

/// A node reached by a traversal and how it was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub node: u32,
    pub depth: u32,
    /// The node it was reached from, the link used and whether that link was followed forward.
    pub parent: Option<(u32, Link, bool)>,
}

#[derive(Debug, Default)]
struct Adjacency {
    offsets: Vec<u32>,
    links: Vec<Link>,
}

impl Adjacency {
    fn build(node_count: usize, mut pairs: Vec<(u32, Link)>) -> Self {
        let mut offsets = vec![0u32; node_count + 1];
        for (node, _) in &pairs {
            offsets[*node as usize + 1] += 1;
        }
        for index in 0..node_count {
            offsets[index + 1] += offsets[index];
        }
        pairs.sort_by_key(|(node, _)| *node);
        let links = pairs.into_iter().map(|(_, link)| link).collect();
        Adjacency { offsets, links }
    }

    fn of(&self, node: u32) -> &[Link] {
        let start = self.offsets[node as usize] as usize;
        let end = self.offsets[node as usize + 1] as usize;
        &self.links[start..end]
    }
}

/// An immutable slice of the graph with dense local node indices.
#[derive(Debug)]
pub struct Csr {
    ids: Vec<SymbolId>,
    index: HashMap<SymbolId, u32>,
    outgoing: Adjacency,
    incoming: Adjacency,
    truncated: bool,
}

impl Csr {
    /// Builds a slice from edges. `truncated` records that the loader stopped early.
    pub fn build(edges: &[SliceEdge], truncated: bool) -> Self {
        let mut ids = Vec::new();
        let mut index = HashMap::new();
        let mut local_of = |id: SymbolId| -> u32 {
            *index.entry(id).or_insert_with(|| {
                ids.push(id);
                (ids.len() - 1) as u32
            })
        };
        let mut outgoing = Vec::with_capacity(edges.len());
        let mut incoming = Vec::with_capacity(edges.len());
        for edge in edges {
            let src = local_of(edge.src);
            let dst = local_of(edge.dst);
            let link = |other| Link {
                other,
                kind: edge.kind,
                provenance: edge.provenance,
                line: edge.line,
            };
            outgoing.push((src, link(dst)));
            incoming.push((dst, link(src)));
        }
        let node_count = ids.len();
        Csr {
            ids,
            index,
            outgoing: Adjacency::build(node_count, outgoing),
            incoming: Adjacency::build(node_count, incoming),
            truncated,
        }
    }

    pub fn node_count(&self) -> usize {
        self.ids.len()
    }

    /// True when the slice was cut at the node cap, so traversals may miss nodes.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn local(&self, id: SymbolId) -> Option<u32> {
        self.index.get(&id).copied()
    }

    pub fn id(&self, local: u32) -> SymbolId {
        self.ids[local as usize]
    }

    /// Every node within `max_depth` hops of `start`, nearest first, including `start` at depth 0.
    pub fn reach(&self, start: u32, direction: Direction, max_depth: u32) -> Vec<Visit> {
        self.search(start, direction, max_depth, None).0
    }

    /// The fewest-hop path from `from` to `to` as visits in order, `from` first.
    pub fn shortest_path(
        &self,
        from: u32,
        to: u32,
        direction: Direction,
        max_depth: u32,
    ) -> Option<Vec<Visit>> {
        let (visits, slot) = self.search(from, direction, max_depth, Some(to));
        let mut position = slot[to as usize]?;
        let mut path = vec![visits[position]];
        while let Some((parent, _, _)) = visits[position].parent {
            position = slot[parent as usize].expect("a parent was visited before its child");
            path.push(visits[position]);
        }
        path.reverse();
        Some(path)
    }

    /// Breadth-first search; stops early once `target` is visited. Also returns, for each local
    /// node, its position in the visit list.
    fn search(
        &self,
        start: u32,
        direction: Direction,
        max_depth: u32,
        target: Option<u32>,
    ) -> (Vec<Visit>, Vec<Option<usize>>) {
        let mut slot = vec![None; self.node_count()];
        let mut visits = vec![Visit {
            node: start,
            depth: 0,
            parent: None,
        }];
        slot[start as usize] = Some(0);
        let mut queue = VecDeque::from([0usize]);
        while let Some(position) = queue.pop_front() {
            let current = visits[position];
            if target == Some(current.node) {
                break;
            }
            if current.depth >= max_depth {
                continue;
            }
            for followed in direction.directions() {
                let adjacency = match followed {
                    Direction::In => &self.incoming,
                    _ => &self.outgoing,
                };
                for link in adjacency.of(current.node) {
                    if slot[link.other as usize].is_some() {
                        continue;
                    }
                    slot[link.other as usize] = Some(visits.len());
                    queue.push_back(visits.len());
                    visits.push(Visit {
                        node: link.other,
                        depth: current.depth + 1,
                        parent: Some((current.node, *link, *followed == Direction::Out)),
                    });
                }
            }
        }
        (visits, slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> SymbolId {
        SymbolId::derive(crate::NodeKind::Function, name, "f.rs")
    }

    fn call(from: &str, to: &str) -> SliceEdge {
        SliceEdge {
            src: id(from),
            dst: id(to),
            kind: EdgeKind::Calls,
            provenance: Provenance::Extracted,
            line: None,
        }
    }

    fn chain() -> Csr {
        // a -> b -> c -> d, a -> d, e -> b
        let edges = [
            call("a", "b"),
            call("b", "c"),
            call("c", "d"),
            call("a", "d"),
            call("e", "b"),
        ];
        Csr::build(&edges, false)
    }

    fn names_at(csr: &Csr, visits: &[Visit]) -> Vec<SymbolId> {
        visits.iter().map(|visit| csr.id(visit.node)).collect()
    }

    #[test]
    fn reach_follows_direction_and_depth() {
        let csr = chain();
        let start = csr.local(id("a")).unwrap();
        let forward = csr.reach(start, Direction::Out, 1);
        assert_eq!(names_at(&csr, &forward), [id("a"), id("b"), id("d")]);
        let callers = csr.reach(csr.local(id("b")).unwrap(), Direction::In, 5);
        assert_eq!(names_at(&csr, &callers), [id("b"), id("a"), id("e")]);
    }

    #[test]
    fn shortest_path_takes_fewest_hops() {
        let csr = chain();
        let from = csr.local(id("a")).unwrap();
        let to = csr.local(id("d")).unwrap();
        let path = csr.shortest_path(from, to, Direction::Out, 5).unwrap();
        assert_eq!(names_at(&csr, &path), [id("a"), id("d")]);
        assert!(csr.shortest_path(to, from, Direction::Out, 5).is_none());
        let undirected = csr.shortest_path(to, from, Direction::Both, 5).unwrap();
        assert_eq!(names_at(&csr, &undirected), [id("d"), id("a")]);
        assert!(!undirected[1].parent.unwrap().2);
    }

    #[test]
    fn path_respects_depth_limit() {
        let csr = chain();
        let from = csr.local(id("e")).unwrap();
        let to = csr.local(id("d")).unwrap();
        assert!(csr.shortest_path(from, to, Direction::Out, 2).is_none());
        assert_eq!(
            csr.shortest_path(from, to, Direction::Out, 3)
                .unwrap()
                .len(),
            4
        );
    }
}
