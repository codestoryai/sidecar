// use std::arch::aarch64::*;
// use std::collections::HashMap;

// struct Graph {
//     nodes: Vec<Node>,
//     edges: Vec<Vec<usize>>,
// }

// struct Node {
//     id: usize,
//     rank: f32,
// }

// impl Graph {
//     fn new(num_nodes: usize) -> Self {
//         Graph {
//             nodes: (0..num_nodes)
//                 .map(|id| Node {
//                     id,
//                     rank: 1.0 / num_nodes as f32,
//                 })
//                 .collect(),
//             edges: vec![Vec::new(); num_nodes],
//         }
//     }

//     fn add_edge(&mut self, from: usize, to: usize) {
//         self.edges[from].push(to);
//     }
// }

// struct PageRank {
//     graph: Graph,
//     damping_factor: f32,
//     epsilon: f32,
//     personalization: Vec<f32>,
//     max_iterations: usize,
// }

// impl PageRank {
//     fn new(
//         num_nodes: usize,
//         damping_factor: f32,
//         epsilon: f32,
//         personalization: Vec<f32>,
//         max_iterations: usize,
//     ) -> Self {
//         PageRank {
//             graph: Graph::new(num_nodes),
//             damping_factor,
//             epsilon,
//             personalization,
//             max_iterations,
//         }
//     }

//     fn add_edge(&mut self, from: usize, to: usize) {
//         self.graph.add_edge(from, to);
//     }

//     fn calculate(&mut self) -> usize {
//         let n = self.graph.nodes.len();
//         let mut new_ranks = vec![0.0f32; n];
//         let mut iterations = 0;

//         unsafe {
//             let damping_factor = vdupq_n_f32(self.damping_factor);
//             let one_minus_damping = vdupq_n_f32(1.0 - self.damping_factor);

//             loop {
//                 let mut max_diff = 0.0f32;

//                 for i in 0..n {
//                     let mut sum = vdupq_n_f32(0.0);
//                     let chunks = n / 4;

//                     // SIMD processing for chunks of 4
//                     for chunk in 0..chunks {
//                         let base = chunk * 4;
//                         let mut contrib = vdupq_n_f32(0.0);
//                         let _ = vld1q_f32(self.graph.nodes[base..].as_ptr() as *const f32);

//                         if self.graph.edges[base].contains(&i) {
//                             let out_degree = self.graph.edges[base].len() as f32;
//                             contrib = vsetq_lane_f32(
//                                 self.graph.nodes[base].rank / out_degree,
//                                 contrib,
//                                 0,
//                             );
//                         }
//                         if self.graph.edges[base + 1].contains(&i) {
//                             let out_degree = self.graph.edges[base + 1].len() as f32;
//                             contrib = vsetq_lane_f32(
//                                 self.graph.nodes[base + 1].rank / out_degree,
//                                 contrib,
//                                 1,
//                             );
//                         }
//                         if self.graph.edges[base + 2].contains(&i) {
//                             let out_degree = self.graph.edges[base + 2].len() as f32;
//                             contrib = vsetq_lane_f32(
//                                 self.graph.nodes[base + 2].rank / out_degree,
//                                 contrib,
//                                 2,
//                             );
//                         }
//                         if self.graph.edges[base + 3].contains(&i) {
//                             let out_degree = self.graph.edges[base + 3].len() as f32;
//                             contrib = vsetq_lane_f32(
//                                 self.graph.nodes[base + 3].rank / out_degree,
//                                 contrib,
//                                 3,
//                             );
//                         }

//                         sum = vaddq_f32(sum, vmulq_f32(damping_factor, contrib));
//                     }

//                     let mut sum_array = [0.0f32; 4];
//                     vst1q_f32(sum_array.as_mut_ptr(), sum);
//                     let mut total_sum = sum_array.iter().sum::<f32>();

//                     // Handle remaining nodes
//                     for j in (chunks * 4)..n {
//                         if self.graph.edges[j].contains(&i) {
//                             let out_degree = self.graph.edges[j].len() as f32;
//                             total_sum +=
//                                 self.damping_factor * self.graph.nodes[j].rank / out_degree;
//                         }
//                     }

//                     // Correct application of one_minus_damping
//                     let random_jump =
//                         vgetq_lane_f32(one_minus_damping, 0) * self.personalization[i];
//                     new_ranks[i] = total_sum + random_jump;

//                     max_diff = max_diff.max((new_ranks[i] - self.graph.nodes[i].rank).abs());
//                 }

//                 // Update ranks
//                 for i in 0..n {
//                     self.graph.nodes[i].rank = new_ranks[i];
//                 }

//                 iterations += 1;

//                 if max_diff < self.epsilon || iterations >= self.max_iterations {
//                     break;
//                 }
//             }
//         }

//         iterations
//     }

//     fn get_ranks(&self) -> HashMap<usize, f32> {
//         self.graph
//             .nodes
//             .iter()
//             .map(|node| (node.id, node.rank))
//             .collect()
//     }
// }

// fn main() {
//     let num_nodes = 1000; // Example with a larger number of nodes
//     let personalization = vec![1.0 / num_nodes as f32; num_nodes]; // Equal personalization
//     let mut pr = PageRank::new(num_nodes, 0.85, 0.0001, personalization, 100);

//     // Add some example edges (you'd typically have many more for a large graph)
//     for i in 0..num_nodes {
//         pr.add_edge(i, (i + 1) % num_nodes);
//         pr.add_edge(i, (i + 10) % num_nodes);
//     }

//     pr.calculate();

//     let ranks = pr.get_ranks();
//     println!("Number of nodes: {}", num_nodes);
//     println!(
//         "PageRank of first 100 nodes: {:?}",
//         ranks.iter().take(100).collect::<HashMap<_, _>>()
//     );
// }

#[tokio::main]
async fn main() {
    println!("disabled it for now cause of architecture issues with using simd");
}
