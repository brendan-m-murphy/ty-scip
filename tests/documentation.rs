use std::{fs, path::PathBuf, process::Command};

use scip::types::{Document, SymbolInformation};

mod support;

fn symbol<'a>(document: &'a Document, name: &str) -> &'a SymbolInformation {
    document
        .symbols
        .iter()
        .find(|symbol| symbol.display_name == name)
        .unwrap_or_else(|| panic!("missing symbol metadata for {name}"))
}

fn signature(symbol: &SymbolInformation) -> Option<(&str, &str)> {
    symbol
        .signature_documentation
        .as_ref()
        .map(|signature| (signature.language.as_str(), signature.text.as_str()))
}

#[test]
fn emits_source_docs_and_deterministic_syntax_signatures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/documentation");
    let index =
        std::env::temp_dir().join(format!("ty-scip-documentation-{}.scip", std::process::id()));
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_ty-scip"))
            .arg(&root)
            .arg(&index)
            .output()
            .expect("run ty-scip")
    };

    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let first = fs::read(&index).expect("read SCIP index");
    let decoded = support::read_index(&index);
    let document = support::document(&decoded, "library.py");

    assert_eq!(
        symbol(document, "library").documentation,
        ["Module documentation."]
    );
    assert_eq!(
        symbol(document, "SETTING").documentation,
        ["Attribute documentation."]
    );

    let example = symbol(document, "Example");
    assert_eq!(example.documentation, ["Class documentation."]);
    assert_eq!(
        signature(example),
        Some(("python", "class Example(object)"))
    );

    let fetch = symbol(document, "fetch");
    assert_eq!(fetch.documentation, ["Async method documentation."]);
    assert_eq!(
        signature(fetch),
        Some((
            "python",
            "async def fetch(\n        value: int,\n        enabled: bool = True,\n    ) -> str",
        ))
    );

    let choose = symbol(document, "choose");
    assert_eq!(choose.documentation, ["First overload documentation."]);
    assert_eq!(
        signature(choose),
        Some(("python", "def choose(value: int) -> int"))
    );

    let property = document
        .symbols
        .iter()
        .find(|symbol| {
            symbol.display_name == "value"
                && signature(symbol).is_some_and(|(_, text)| text.starts_with("def value("))
        })
        .expect("property method metadata");
    assert_eq!(property.documentation, ["Getter documentation."]);
    assert_eq!(
        signature(property),
        Some(("python", "def value(self) -> int"))
    );

    let inner = symbol(document, "inner");
    assert_eq!(inner.documentation, ["Nested documentation."]);
    assert_eq!(
        signature(inner),
        Some(("python", "def inner(value: int) -> int"))
    );
    assert_eq!(
        support::occurrence(document, &[51, 8, 13]).symbol,
        inner.symbol
    );

    let second = run();
    assert!(second.status.success());
    assert_eq!(first, fs::read(&index).expect("reread SCIP index"));
    fs::remove_file(index).expect("remove test index");
}
