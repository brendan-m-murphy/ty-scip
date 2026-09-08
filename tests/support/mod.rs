use std::{fs, path::Path};

use protobuf::Message;
use scip::types::{Document, Index, Occurrence};

pub fn read_index(path: &Path) -> Index {
    Index::parse_from_bytes(&fs::read(path).expect("read SCIP index")).expect("decode SCIP index")
}

pub fn document<'a>(index: &'a Index, path: &str) -> &'a Document {
    index
        .documents
        .iter()
        .find(|document| document.relative_path == path)
        .unwrap_or_else(|| panic!("missing SCIP document {path}"))
}

pub fn occurrence<'a>(document: &'a Document, range: &[i32]) -> &'a Occurrence {
    document
        .occurrences
        .iter()
        .find(|occurrence| occurrence.range == range)
        .unwrap_or_else(|| panic!("missing SCIP occurrence {range:?}"))
}
