//! Project a lossless SCIP index into Graphify's JSON graph shape.
//!
//! SCIP occurrences remain the source of truth here.  In particular, this
//! module does not collapse ranges into chunks or try to infer call edges:
//! an occurrence is a reference, import, or write according to its SCIP role.

pub mod graphify;
