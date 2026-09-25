//! KiCad integration boundary.
//!
//! The first implementation will invoke KiCad's command-line exporter and
//! convert its output into [`crate::ConnectivityGraph`].

use crate::ConnectivityGraph;

/// Placeholder for the KiCad-to-graph pipeline.
pub fn load_connectivity_graph() -> ConnectivityGraph {
    ConnectivityGraph::default()
}
