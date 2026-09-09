//! Project SCIP evidence into Graphify's JSON graph shape.
//!
//! The `.scip` file remains the complete source of truth. This projection
//! preserves occurrence evidence and useful symbol metadata without trying to
//! infer call edges: an occurrence is a reference, import, or write according
//! to its SCIP role.

pub mod graphify;
