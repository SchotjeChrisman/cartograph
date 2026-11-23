//! Leiden community detection (Traag, Waltman and van Eck, 2019) on a [`WeightedGraph`], with
//! modularity at a caller-chosen resolution as the quality function.
//!
//! One deterministic pass: nodes are visited in index order and ties go to the lowest label, so
//! the same graph always gives the same partition. The paper's refinement step picks a target
//! at random; here it picks the best gain, which keeps runs reproducible at the cost of a little
//! exploration.

use std::collections::VecDeque;

use crate::weighted::WeightedGraph;

/// Gains closer to zero than this are noise from floating-point sums.
const EPSILON: f64 = 1e-12;

/// Partitions `graph` starting from `initial` (one label per node, labels below `graph.len()`).
/// Higher `resolution` gives smaller communities. `doubled_total` is twice the edge weight of
/// the whole graph the null model refers to; for a subgraph it is the parent's, so a resolution
/// means the same at every level. Returns labels renumbered from 0.
pub fn partition(
    graph: &WeightedGraph,
    initial: &[usize],
    resolution: f64,
    doubled_total: f64,
) -> Vec<usize> {
    let mut labels = initial.to_vec();
    if doubled_total <= 0.0 {
        renumber(&mut labels);
        return labels;
    }
    leiden(graph, labels, resolution, doubled_total)
}

fn leiden(
    graph: &WeightedGraph,
    mut labels: Vec<usize>,
    resolution: f64,
    doubled_total: f64,
) -> Vec<usize> {
    renumber(&mut labels);
    move_nodes(graph, &mut labels, resolution, doubled_total);
    let community_count = renumber(&mut labels);
    if community_count == graph.len() {
        return labels;
    }
    let mut refined = refine(graph, &labels, resolution, doubled_total);
    let refined_count = renumber(&mut refined);
    if refined_count == graph.len() {
        return labels;
    }
    let aggregate = graph.aggregate(&refined, refined_count);
    let mut aggregate_labels = vec![0; refined_count];
    for (node, refined_label) in refined.iter().enumerate() {
        aggregate_labels[*refined_label] = labels[node];
    }
    let merged = leiden(&aggregate, aggregate_labels, resolution, doubled_total);
    let mut result: Vec<usize> = refined.iter().map(|label| merged[*label]).collect();
    renumber(&mut result);
    result
}

/// Rewrites labels to `0..count` in order of first appearance and returns the count.
fn renumber(labels: &mut [usize]) -> usize {
    let mut next = 0;
    let mut mapping = std::collections::HashMap::new();
    for label in labels.iter_mut() {
        *label = *mapping.entry(*label).or_insert_with(|| {
            next += 1;
            next - 1
        });
    }
    next
}

/// Weight from one node to each neighbouring community, with cheap reset between nodes.
struct NeighbourWeights {
    weight: Vec<f64>,
    touched: Vec<usize>,
}

impl NeighbourWeights {
    fn new(size: usize) -> Self {
        NeighbourWeights {
            weight: vec![0.0; size],
            touched: Vec::new(),
        }
    }

    fn add(&mut self, community: usize, weight: f64) {
        if self.weight[community] == 0.0 {
            self.touched.push(community);
        }
        self.weight[community] += weight;
    }

    fn clear(&mut self) {
        for community in self.touched.drain(..) {
            self.weight[community] = 0.0;
        }
    }
}

/// Local moving: each node joins the neighbouring community with the best modularity gain,
/// until a full sweep changes nothing.
fn move_nodes(graph: &WeightedGraph, labels: &mut [usize], resolution: f64, doubled_total: f64) {
    let strengths: Vec<f64> = (0..graph.len()).map(|node| graph.strength(node)).collect();
    let mut community_strength = vec![0.0; graph.len()];
    for (node, label) in labels.iter().enumerate() {
        community_strength[*label] += strengths[node];
    }
    let mut queue: VecDeque<usize> = (0..graph.len()).collect();
    let mut queued = vec![true; graph.len()];
    let mut weights = NeighbourWeights::new(graph.len());
    while let Some(node) = queue.pop_front() {
        queued[node] = false;
        let own = labels[node];
        for (neighbour, weight) in graph.neighbours(node) {
            weights.add(labels[*neighbour], *weight);
        }
        community_strength[own] -= strengths[node];
        let gain = |community: usize, weights: &NeighbourWeights| {
            weights.weight[community]
                - resolution * community_strength[community] * strengths[node] / doubled_total
        };
        let mut best = own;
        let mut best_gain = gain(own, &weights);
        for community in &weights.touched {
            let candidate = gain(*community, &weights);
            if candidate > best_gain + EPSILON {
                best = *community;
                best_gain = candidate;
            }
        }
        community_strength[best] += strengths[node];
        labels[node] = best;
        weights.clear();
        if best != own {
            for (neighbour, _) in graph.neighbours(node) {
                if labels[*neighbour] != best && !queued[*neighbour] {
                    queued[*neighbour] = true;
                    queue.push_back(*neighbour);
                }
            }
        }
    }
}

/// Splits each community into well-connected sub-communities by merging singletons, which is
/// what keeps Leiden communities connected where Louvain's can fall apart.
fn refine(
    graph: &WeightedGraph,
    labels: &[usize],
    resolution: f64,
    doubled_total: f64,
) -> Vec<usize> {
    let node_count = graph.len();
    let strengths: Vec<f64> = (0..node_count).map(|node| graph.strength(node)).collect();
    let mut community_strength = vec![0.0; node_count];
    for (node, label) in labels.iter().enumerate() {
        community_strength[*label] += strengths[node];
    }
    let mut refined: Vec<usize> = (0..node_count).collect();
    let mut refined_strength = strengths.clone();
    let mut refined_size = vec![1usize; node_count];
    // Weight between a refined community and the rest of its parent community.
    let mut outside_weight: Vec<f64> = (0..node_count)
        .map(|node| {
            graph
                .neighbours(node)
                .iter()
                .filter(|(other, _)| labels[*other] == labels[node])
                .map(|(_, weight)| weight)
                .sum()
        })
        .collect();
    let well_connected = |outside: f64, strength: f64, parent: f64| {
        outside >= resolution * strength * (parent - strength) / doubled_total - EPSILON
    };
    let mut weights = NeighbourWeights::new(node_count);
    for node in 0..node_count {
        let parent = labels[node];
        let is_singleton = refined_size[refined[node]] == 1;
        if !is_singleton
            || !well_connected(
                outside_weight[node],
                strengths[node],
                community_strength[parent],
            )
        {
            continue;
        }
        for (neighbour, weight) in graph.neighbours(node) {
            if labels[*neighbour] == parent {
                weights.add(refined[*neighbour], *weight);
            }
        }
        let mut best: Option<(usize, f64)> = None;
        for target in &weights.touched {
            if !well_connected(
                outside_weight[*target],
                refined_strength[*target],
                community_strength[parent],
            ) {
                continue;
            }
            let gain = weights.weight[*target]
                - resolution * refined_strength[*target] * strengths[node] / doubled_total;
            if gain >= 0.0 && best.is_none_or(|(_, best_gain)| gain > best_gain + EPSILON) {
                best = Some((*target, gain));
            }
        }
        if let Some((target, _)) = best {
            outside_weight[target] += outside_weight[node] - 2.0 * weights.weight[target];
            refined_strength[target] += strengths[node];
            refined_size[target] += 1;
            refined_size[node] = 0;
            refined[node] = target;
        }
        weights.clear();
    }
    refined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clique(graph: &mut WeightedGraph, nodes: &[usize]) {
        for (index, a) in nodes.iter().enumerate() {
            for b in &nodes[index + 1..] {
                graph.add_edge(*a, *b, 1.0);
            }
        }
    }

    fn singletons(count: usize) -> Vec<usize> {
        (0..count).collect()
    }

    #[test]
    fn two_cliques_joined_by_one_edge_split_in_two() {
        let mut graph = WeightedGraph::with_nodes(10);
        clique(&mut graph, &[0, 1, 2, 3, 4]);
        clique(&mut graph, &[5, 6, 7, 8, 9]);
        graph.add_edge(4, 5, 1.0);

        let labels = partition(&graph, &singletons(10), 1.0, graph.doubled_total());

        assert!(labels[..5].iter().all(|label| *label == labels[0]));
        assert!(labels[5..].iter().all(|label| *label == labels[5]));
        assert_ne!(labels[0], labels[5]);
    }

    #[test]
    fn higher_resolution_gives_more_communities() {
        let mut graph = WeightedGraph::with_nodes(12);
        for group in 0..4 {
            clique(&mut graph, &[group * 3, group * 3 + 1, group * 3 + 2]);
        }
        for group in 0..3 {
            graph.add_edge(group * 3, group * 3 + 3, 1.0);
        }
        let count = |resolution| {
            let mut labels = partition(&graph, &singletons(12), resolution, graph.doubled_total());
            renumber(&mut labels)
        };

        assert!(count(0.05) < count(3.0));
        assert_eq!(count(3.0), 4);
    }

    #[test]
    fn a_graph_without_edges_keeps_its_seed() {
        let graph = WeightedGraph::with_nodes(3);
        assert_eq!(
            partition(&graph, &[0, 0, 1], 1.0, graph.doubled_total()),
            vec![0, 0, 1]
        );
    }

    #[test]
    fn the_result_is_reproducible() {
        let mut graph = WeightedGraph::with_nodes(8);
        clique(&mut graph, &[0, 1, 2, 3]);
        clique(&mut graph, &[4, 5, 6, 7]);
        graph.add_edge(3, 4, 1.0);
        graph.add_edge(0, 7, 1.0);
        let run = || partition(&graph, &singletons(8), 1.0, graph.doubled_total());
        assert_eq!(run(), run());
    }
}
