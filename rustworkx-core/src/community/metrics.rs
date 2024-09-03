use super::utils::total_edge_weight;
use super::NotAPartitionError;

use petgraph::visit::{
    Data, EdgeRef, GraphProp, IntoEdgeReferences, IntoNodeReferences, NodeCount, NodeIndexable,
};
use std::collections::HashSet;
use std::hash::Hash;

pub trait ModularityComputable:
    Data<EdgeWeight: Into<f64> + Copy, NodeId: Hash + Eq + Copy>
    + GraphProp
    + IntoEdgeReferences
    + NodeCount
    + IntoNodeReferences
    + NodeIndexable
{
}
impl<
        Graph: Data<EdgeWeight: Into<f64> + Copy, NodeId: Hash + Eq + Copy>
            + GraphProp
            + IntoEdgeReferences
            + NodeCount
            + IntoNodeReferences
            + NodeIndexable,
    > ModularityComputable for Graph
{
}

// pub fn index_map_from_subsets<N>(subsets: &[HashSet<N>]) -> HashMap<N, usize>
// where
//     N: Hash + Copy + Eq,
// {
//     let mut h = HashMap::with_capacity(subsets.iter().map(|s| s.len()).sum());
//     for (ii, s) in subsets.iter().enumerate() {
//         for &n in s {
//             h.insert(n, ii);
//         }
//     }
//     h
// }

pub struct Partition<'g, G>
where
    G: ModularityComputable,
{
    graph: &'g G,
    n_subsets: usize,
    pub node_to_subset: Vec<usize>,
}
struct PartitionEdgeWeights {
    pub internal: Vec<f64>,
    pub outgoing: Vec<f64>,
    pub incoming: Option<Vec<f64>>,
}

impl<'g, G: ModularityComputable> Partition<'g, G> {
    pub fn new(
        graph: &'g G,
        subsets: &[HashSet<G::NodeId>],
    ) -> Result<Partition<'g, G>, NotAPartitionError> {
        // Move this into a separate helper function
        let mut seen = vec![false; graph.node_count()];

        let mut node_to_subset = vec![0; graph.node_count()];

        for (ii, v) in subsets.iter().enumerate() {
            for &node in v {
                let idx = graph.to_index(node);
                if seen[idx] {
                    // argument `communities` contains a duplicate node
                    return Err(NotAPartitionError {});
                }
                node_to_subset[idx] = ii;
                seen[idx] = true;
            }
        }

        if !seen.iter().all(|&t| t) {
            return Err(NotAPartitionError {});
        }

        Ok(Partition::<'g, G> {
            graph: graph,
            n_subsets: subsets.len(),
            node_to_subset: node_to_subset,
        })
    }

    pub fn get_subset_id(&self, node: G::NodeId) -> usize {
        let idx = self.graph.to_index(node);
        self.node_to_subset[idx]
    }

    fn partition_edge_weights(&self) -> PartitionEdgeWeights {
        let mut internal_edge_weights = vec![0.0; self.n_subsets];
        let mut outgoing_edge_weights = vec![0.0; self.n_subsets];

        let directed = self.graph.is_directed();
        let mut incoming_edge_weights = if directed {
            Some(vec![0.0; self.n_subsets])
        } else {
            None
        };

        for edge in self.graph.edge_references() {
            let (a, b) = (edge.source(), edge.target());
            let (c_a, c_b) = (self.get_subset_id(a), self.get_subset_id(b));
            let w: f64 = (*edge.weight()).into();
            if c_a == c_b {
                internal_edge_weights[c_a] += w;
            }
            outgoing_edge_weights[c_a] += w;
            if let Some(ref mut incoming) = incoming_edge_weights {
                incoming[c_b] += w;
            } else {
                outgoing_edge_weights[c_b] += w;
            }
        }

        PartitionEdgeWeights {
            internal: internal_edge_weights,
            outgoing: outgoing_edge_weights,
            incoming: incoming_edge_weights,
        }
    }

    pub fn modularity(&self, resolution: f64) -> f64 {
        let weights = self.partition_edge_weights();

        let sigma_internal: f64 = weights.internal.iter().sum();

        let sigma_total_squared: f64 = if let Some(incoming) = weights.incoming {
            incoming
                .iter()
                .zip(weights.outgoing.iter())
                .map(|(&x, &y)| x * y)
                .sum()
        } else {
            weights.outgoing.iter().map(|&x| x * x).sum::<f64>() / 4.0
        };

        let m: f64 = total_edge_weight(self.graph);
        sigma_internal / m - resolution * sigma_total_squared / (m * m)
    }
}

pub fn modularity<G>(
    graph: G,
    communities: &[HashSet<G::NodeId>],
    resolution: f64,
) -> Result<f64, NotAPartitionError>
where
    G: ModularityComputable,
{
    let partition = Partition::new(&graph, &communities)?;

    Ok(partition.modularity(resolution))
}

#[cfg(test)]
mod tests {
    use crate::generators::barbell_graph;
    use petgraph::graph::{DiGraph, UnGraph};
    use petgraph::visit::{GraphBase, IntoNodeIdentifiers};
    use std::collections::HashSet;

    use super::modularity;

    #[test]
    fn test_modularity_barbell_graph() {
        type G = UnGraph<(), f64>;
        type N = <G as GraphBase>::NodeId;

        for n in 3..10 {
            let g: G = barbell_graph(Some(n), Some(0), None, None, || (), || 1.0f64).unwrap();
            let nodes: Vec<N> = g.node_identifiers().collect();
            let communities: Vec<HashSet<N>> = vec![
                (0..n).map(|ii| nodes[ii]).collect(),
                (n..(2 * n)).map(|ii| nodes[ii]).collect(),
            ];
            let resolution = 1.0;
            let m = modularity(&g, &communities, resolution).unwrap();
            // There are two complete subgraphs, each with:
            //     * e = n*(n-1)/2 internal edges
            //     * total node degree 2*e + 1
            // The edge weight for the whole graph is 2*e + 1. So the expected
            // modularity is 2 * [ e/(2*e + 1) - 1/4 ].
            let e = (n * (n - 1) / 2) as f64;
            let m_expected = 2.0 * (e / (2.0 * e + 1.0) - 0.25);
            assert!((m - m_expected).abs() < 1.0e-9);
        }
    }

    #[test]
    fn test_modularity_directed() {
        type G = DiGraph<(), f64>;
        type N = <G as GraphBase>::NodeId;

        for n in 3..10 {
            let mut g = G::with_capacity(2 * n, 2 * n + 2);
            for _ii in 0..2 * n {
                g.add_node(());
            }
            let nodes: Vec<N> = g.node_identifiers().collect();
            // Create two cycles
            for ii in 0..n {
                let jj = (ii + 1) % n;
                g.add_edge(nodes[ii], nodes[jj], 1.0);
                g.add_edge(nodes[n + ii], nodes[n + jj], 1.0);
            }
            // Add two edges connecting the cycles
            g.add_edge(nodes[0], nodes[n], 1.0);
            g.add_edge(nodes[n + 1], nodes[1], 1.0);

            let communities: Vec<HashSet<N>> = vec![
                (0..n).map(|ii| nodes[ii]).collect(),
                (n..2 * n).map(|ii| nodes[ii]).collect(),
            ];

            let resolution = 1.0;
            let m = modularity(&g, &communities, resolution).unwrap();

            // Each cycle subgraph has:
            //     * n internal edges
            //     * total node degree n + 1 (outgoing) and n + 1 (incoming)
            // The edge weight for the whole graph is 2*n + 2. So the expected
            // modularity is 2 * [ n/(2*n + 2) - (n+1)^2 / (2*n + 2)^2 ]
            //               = n/(n + 1) - 1/2
            let m_expected = n as f64 / (n as f64 + 1.0) - 0.5;
            assert!((m - m_expected).abs() < 1.0e-9);
        }
    }
}
