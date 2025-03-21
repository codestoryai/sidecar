//! Generates feedback for the action which the node can take
//! This makes sure that we do not end up taking the same trajectories again and again
//! and try to diversify in our search space

pub(crate) mod error;
pub(crate) mod feedback;
