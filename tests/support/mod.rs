use std::{collections::HashSet, fs, path::Path};

use protobuf::Message;
use scip::{
    symbol::is_local_symbol,
    types::{Document, Index, Occurrence, SymbolRole},
};

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

pub fn assert_index_integrity(index: &Index) {
    let document_symbols: HashSet<_> = index
        .documents
        .iter()
        .flat_map(|document| document.symbols.iter().map(|symbol| symbol.symbol.as_str()))
        .collect();
    let external_symbols: HashSet<_> = index
        .external_symbols
        .iter()
        .map(|symbol| symbol.symbol.as_str())
        .collect();
    let definition_symbols: HashSet<_> = index
        .documents
        .iter()
        .flat_map(|document| &document.occurrences)
        .filter(|occurrence| occurrence.symbol_roles & SymbolRole::Definition as i32 != 0)
        .map(|occurrence| occurrence.symbol.as_str())
        .collect();
    for document in &index.documents {
        let symbols: HashSet<_> = document
            .symbols
            .iter()
            .map(|symbol| symbol.symbol.as_str())
            .collect();

        for occurrence in &document.occurrences {
            if occurrence.symbol_roles & SymbolRole::Definition as i32 != 0
                && !is_local_symbol(&occurrence.symbol)
            {
                assert!(
                    symbols.contains(occurrence.symbol.as_str()),
                    "global definition {} in {} has no document symbol information",
                    occurrence.symbol,
                    document.relative_path
                );
            }
        }
    }

    for relationship in index
        .documents
        .iter()
        .flat_map(|document| &document.symbols)
        .chain(&index.external_symbols)
        .flat_map(|symbol| &symbol.relationships)
    {
        assert!(
            (document_symbols.contains(relationship.symbol.as_str())
                && definition_symbols.contains(relationship.symbol.as_str()))
                || external_symbols.contains(relationship.symbol.as_str()),
            "relationship target {} has no definition and symbol information",
            relationship.symbol
        );
    }
}
