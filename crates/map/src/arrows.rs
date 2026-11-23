//! Turning file-to-file links into arrows between the boxes of one level. Pure: no graph access.

use std::collections::HashMap;

use graph::FileLink;
use serde::Serialize;

use crate::node_id::NodeId;

/// Most boxes drawn at once; the rest are counted, not drawn.
pub const MAX_BOXES: usize = 50;
/// Most arrows drawn at once, strongest first.
pub const MAX_ARROWS: usize = 120;

/// All edges from the files of one box to the files of another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Arrow {
    pub from: NodeId,
    pub to: NodeId,
    pub count: u64,
}

/// A box that could be drawn, before the cap is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: NodeId,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Visible {
    pub boxes: Vec<NodeId>,
    pub arrows: Vec<Arrow>,
    /// Candidates left out by the box cap.
    pub hidden_boxes: usize,
}

/// Sums the links between files into arrows between the boxes that own them. Links inside one
/// box, and links from or to a file no box owns, are dropped.
pub fn aggregate(links: &[FileLink], owner_of_file: &HashMap<i64, NodeId>) -> Vec<Arrow> {
    let mut sums: HashMap<(NodeId, NodeId), u64> = HashMap::new();
    for link in links {
        let (Some(&from), Some(&to)) = (
            owner_of_file.get(&link.from_file),
            owner_of_file.get(&link.to_file),
        ) else {
            continue;
        };
        if from != to {
            *sums.entry((from, to)).or_default() += link.edge_count;
        }
    }
    sums.into_iter()
        .map(|((from, to), count)| Arrow { from, to, count })
        .collect()
}

/// Keeps the best-connected boxes up to `max_boxes`, then the strongest arrows between them.
/// Ties break by name and id so the picture is the same on every request.
pub fn select_visible(
    candidates: Vec<Candidate>,
    arrows: Vec<Arrow>,
    max_boxes: usize,
    max_arrows: usize,
) -> Visible {
    let mut weight: HashMap<NodeId, u64> = HashMap::new();
    for arrow in &arrows {
        *weight.entry(arrow.from).or_default() += arrow.count;
        *weight.entry(arrow.to).or_default() += arrow.count;
    }
    let mut ranked = candidates;
    ranked.sort_by(|a, b| {
        let weight_of = |candidate: &Candidate| weight.get(&candidate.id).copied().unwrap_or(0);
        weight_of(b)
            .cmp(&weight_of(a))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    let hidden_boxes = ranked.len().saturating_sub(max_boxes);
    ranked.truncate(max_boxes);
    let boxes: Vec<NodeId> = ranked.into_iter().map(|candidate| candidate.id).collect();

    let mut kept: Vec<Arrow> = arrows
        .into_iter()
        .filter(|arrow| boxes.contains(&arrow.from) && boxes.contains(&arrow.to))
        .collect();
    kept.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| (a.from, a.to).cmp(&(b.from, b.to)))
    });
    kept.truncate(max_arrows);
    Visible {
        boxes,
        arrows: kept,
        hidden_boxes,
    }
}

#[cfg(test)]
mod tests {
    use graph::ComponentId;

    use super::*;

    fn component(id: i64) -> NodeId {
        NodeId::Component(ComponentId(id))
    }

    fn link(from_file: i64, to_file: i64, edge_count: u64) -> FileLink {
        FileLink {
            from_file,
            to_file,
            edge_count,
        }
    }

    fn candidate(id: i64, name: &str) -> Candidate {
        Candidate {
            id: component(id),
            name: name.to_string(),
        }
    }

    #[test]
    fn links_sum_per_box_pair_and_inner_or_unowned_links_vanish() {
        let owners = HashMap::from([(1, component(1)), (2, component(1)), (3, component(2))]);
        let links = [link(1, 3, 4), link(2, 3, 5), link(1, 2, 9), link(3, 99, 7)];

        let arrows = aggregate(&links, &owners);

        assert_eq!(
            arrows,
            vec![Arrow {
                from: component(1),
                to: component(2),
                count: 9
            }]
        );
    }

    #[test]
    fn the_box_cap_keeps_the_best_connected_and_counts_the_rest() {
        let candidates = vec![candidate(1, "a"), candidate(2, "b"), candidate(3, "c")];
        let arrows = vec![
            Arrow {
                from: component(3),
                to: component(2),
                count: 10,
            },
            Arrow {
                from: component(1),
                to: component(2),
                count: 1,
            },
        ];

        let visible = select_visible(candidates, arrows, 2, 10);

        assert_eq!(visible.boxes, vec![component(2), component(3)]);
        assert_eq!(visible.hidden_boxes, 1);
        assert_eq!(
            visible.arrows.len(),
            1,
            "the arrow to the hidden box goes too"
        );
        assert_eq!(visible.arrows[0].count, 10);
    }

    #[test]
    fn the_arrow_cap_keeps_the_strongest() {
        let candidates = vec![candidate(1, "a"), candidate(2, "b"), candidate(3, "c")];
        let arrows = vec![
            Arrow {
                from: component(1),
                to: component(2),
                count: 1,
            },
            Arrow {
                from: component(2),
                to: component(3),
                count: 5,
            },
            Arrow {
                from: component(1),
                to: component(3),
                count: 3,
            },
        ];

        let visible = select_visible(candidates, arrows, 10, 2);

        let counts: Vec<u64> = visible.arrows.iter().map(|arrow| arrow.count).collect();
        assert_eq!(counts, vec![5, 3]);
    }

    #[test]
    fn unconnected_boxes_order_by_name() {
        let candidates = vec![candidate(1, "zeta"), candidate(2, "alpha")];

        let visible = select_visible(candidates, Vec::new(), 10, 10);

        assert_eq!(visible.boxes, vec![component(2), component(1)]);
    }
}
