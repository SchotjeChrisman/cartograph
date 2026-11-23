//! An undirected weighted graph in adjacency-list form, the shape Leiden works on.

/// Nodes are `0..len()`. `self_weight` holds loops, which appear after aggregation when a
/// community collapses into one node; a loop counts twice toward the node's strength.
#[derive(Debug, Clone, Default)]
pub struct WeightedGraph {
    adjacency: Vec<Vec<(usize, f64)>>,
    self_weight: Vec<f64>,
}

impl WeightedGraph {
    pub fn with_nodes(count: usize) -> Self {
        WeightedGraph {
            adjacency: vec![Vec::new(); count],
            self_weight: vec![0.0; count],
        }
    }

    pub fn len(&self) -> usize {
        self.adjacency.len()
    }

    /// Adds an undirected edge; parallel edges stay separate and sum up in every computation.
    pub fn add_edge(&mut self, a: usize, b: usize, weight: f64) {
        if a == b {
            self.self_weight[a] += weight;
            return;
        }
        self.adjacency[a].push((b, weight));
        self.adjacency[b].push((a, weight));
    }

    pub fn neighbours(&self, node: usize) -> &[(usize, f64)] {
        &self.adjacency[node]
    }

    /// Sum of incident edge weights, loops counted twice.
    pub fn strength(&self, node: usize) -> f64 {
        self.adjacency[node]
            .iter()
            .map(|(_, weight)| weight)
            .sum::<f64>()
            + 2.0 * self.self_weight[node]
    }

    /// Twice the total edge weight: the sum of all strengths.
    pub fn doubled_total(&self) -> f64 {
        (0..self.len()).map(|node| self.strength(node)).sum()
    }

    /// The subgraph on `members`, renumbered in the order given. Edges to nodes left out become
    /// loops, so every node keeps its strength and the null model of modularity is unchanged.
    pub fn induced(&self, members: &[usize]) -> WeightedGraph {
        let mut local = vec![usize::MAX; self.len()];
        for (index, member) in members.iter().enumerate() {
            local[*member] = index;
        }
        let mut sub = WeightedGraph::with_nodes(members.len());
        for (index, member) in members.iter().enumerate() {
            sub.self_weight[index] = self.self_weight[*member];
            for (other, weight) in &self.adjacency[*member] {
                if local[*other] != usize::MAX {
                    sub.adjacency[index].push((local[*other], *weight));
                } else {
                    sub.self_weight[index] += weight / 2.0;
                }
            }
        }
        sub
    }

    /// Collapses each community of `partition` (labels `0..communities`) into one node.
    pub fn aggregate(&self, partition: &[usize], communities: usize) -> WeightedGraph {
        let mut merged = WeightedGraph::with_nodes(communities);
        let mut between: Vec<std::collections::HashMap<usize, f64>> =
            vec![Default::default(); communities];
        for node in 0..self.len() {
            let community = partition[node];
            merged.self_weight[community] += self.self_weight[node];
            for (other, weight) in &self.adjacency[node] {
                let other_community = partition[*other];
                if other_community == community {
                    // Each inner edge is seen from both ends, so half per sighting.
                    merged.self_weight[community] += weight / 2.0;
                } else {
                    *between[community].entry(other_community).or_default() += weight;
                }
            }
        }
        for (community, targets) in between.into_iter().enumerate() {
            let mut targets: Vec<_> = targets.into_iter().collect();
            targets.sort_by_key(|(target, _)| *target);
            merged.adjacency[community] = targets;
        }
        merged
    }
}
