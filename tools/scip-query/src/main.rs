use std::{env, path::PathBuf, process::ExitCode};

use scip_query::{
    Page, QueryIndex, RefDirection, ReferenceEvidence, ReferenceView, Resolution, SymbolId,
    SymbolView,
};
use serde_json::{Value, json};

const DEFAULT_LIMIT: usize = 50;
const USAGE: &str = r#"Usage:
  scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH]
             [--limit N] COMMAND ...

Commands:
  find QUERY [--path PREFIX]
  at PATH:LINE[:COLUMN]
  definition SELECTOR
  hover SELECTOR
  references SELECTOR [--path PREFIX] [--offset N]
  members SELECTOR
  callers SELECTOR
  callees SELECTOR
  supertypes SELECTOR
  subtypes SELECTOR

Locations are 1-based. Output is deterministic JSON."#;

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    index: PathBuf,
    facts: Option<PathBuf>,
    root: Option<PathBuf>,
    limit: usize,
    command: String,
    argument: String,
    path: Option<String>,
    offset: usize,
}

fn main() -> ExitCode {
    match parse(env::args().skip(1).collect()) {
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(cli)) => match execute(cli) {
            Ok(0) => ExitCode::SUCCESS,
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("scip-query: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("scip-query: {error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse(args: Vec<String>) -> Result<Option<Cli>, String> {
    if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(None);
    }
    let mut index = None;
    let mut facts = None;
    let mut root = None;
    let mut limit = DEFAULT_LIMIT;
    let mut position = 0;
    while position < args.len() && !is_command(&args[position]) {
        match args[position].as_str() {
            "--index" => index = Some(PathBuf::from(value(&args, &mut position, "--index")?)),
            "--facts" => facts = Some(PathBuf::from(value(&args, &mut position, "--facts")?)),
            "--root" => root = Some(PathBuf::from(value(&args, &mut position, "--root")?)),
            "--limit" => limit = positive(&value(&args, &mut position, "--limit")?, "limit")?,
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            argument => return Err(format!("expected a command, found {argument}")),
        }
        position += 1;
    }
    let command = args
        .get(position)
        .filter(|command| is_command(command))
        .cloned()
        .ok_or_else(|| "missing command".to_owned())?;
    position += 1;
    let argument = args
        .get(position)
        .filter(|argument| !argument.starts_with('-'))
        .cloned()
        .ok_or_else(|| format!("{command} requires one argument"))?;
    position += 1;
    let mut path = None;
    let mut offset = 0;
    while position < args.len() {
        match args[position].as_str() {
            "--path" if matches!(command.as_str(), "find" | "references") => {
                path = Some(value(&args, &mut position, "--path")?)
            }
            "--offset" if command == "references" => {
                offset = value(&args, &mut position, "--offset")?
                    .parse()
                    .map_err(|_| "offset must be a non-negative integer".to_owned())?;
            }
            "--limit" => limit = positive(&value(&args, &mut position, "--limit")?, "limit")?,
            option if option.starts_with('-') => {
                return Err(format!("unknown option {option} for {command}"));
            }
            extra => return Err(format!("unexpected argument {extra} for {command}")),
        }
        position += 1;
    }
    Ok(Some(Cli {
        index: index.ok_or_else(|| "missing SCIP index (--index INDEX.scip)".to_owned())?,
        facts,
        root,
        limit,
        command,
        argument,
        path,
        offset,
    }))
}

fn is_command(value: &str) -> bool {
    matches!(
        value,
        "find"
            | "at"
            | "definition"
            | "hover"
            | "references"
            | "members"
            | "callers"
            | "callees"
            | "supertypes"
            | "subtypes"
    )
}

fn value(args: &[String], position: &mut usize, option: &str) -> Result<String, String> {
    *position += 1;
    args.get(*position)
        .cloned()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn positive(value: &str, label: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{label} must be a positive integer"))
}

fn execute(cli: Cli) -> Result<u8, String> {
    let facts = cli.facts.or_else(|| {
        let candidate = cli.index.with_extension("tyfacts");
        candidate.is_file().then_some(candidate)
    });
    let index = match facts.as_ref() {
        Some(facts) => QueryIndex::load_with_facts(&cli.index, facts, cli.root),
        None => QueryIndex::load(&cli.index, cli.root),
    }
    .map_err(|error| error.to_string())?;

    if cli.command == "find" {
        let result = index.find_in(&cli.argument, cli.path.as_deref(), cli.limit);
        emit(&json!({
            "command": "find",
            "path": cli.path,
            "query": cli.argument,
            "result": result,
            "status": "ok",
        }))?;
        return Ok(0);
    }
    if cli.command == "at" {
        let (path, line, column) = parse_location(&cli.argument)?;
        let result = index
            .at(&path, line, column, cli.limit)
            .map_err(|error| error.to_string())?;
        emit(&json!({
            "command": "at",
            "location": {"path": path, "line": line, "column": column},
            "result": result,
            "status": "ok",
        }))?;
        return Ok(0);
    }

    let symbol = match select(&index, &cli.argument, cli.limit) {
        Ok(symbol) => symbol,
        Err(failure) => return emit_selection_failure(&cli.command, &cli.argument, failure),
    };
    let result = match cli.command.as_str() {
        "definition" => {
            let context = index
                .context_with_limit(&symbol.id, 0, 0, cli.limit)
                .map_err(|error| error.to_string())?;
            json!({
                "definitions": context.definitions,
                "snippets": context.snippets,
                "snippet_failures": context.snippet_failures,
            })
        }
        "hover" => {
            let context = index
                .context_with_limit(&symbol.id, 0, 0, cli.limit)
                .map_err(|error| error.to_string())?;
            json!({
                "documentation": context.documentation,
                "signatures": context.signatures,
                "symbol": context.symbol,
            })
        }
        "references" => {
            let mut items = index
                .refs(&symbol.id, RefDirection::Incoming, usize::MAX)
                .items;
            if let Some(prefix) = cli.path.as_deref() {
                items.retain(|reference| {
                    reference_document(reference)
                        .is_some_and(|document| document.starts_with(prefix))
                });
            }
            let total = items.len();
            let items = items
                .iter()
                .skip(cli.offset)
                .take(cli.limit)
                .map(|reference| compact_reference(&index, reference))
                .collect::<Vec<_>>();
            let returned = items.len();
            let next_offset = (cli.offset + returned < total).then_some(cli.offset + returned);
            json!({
                "items": items,
                "next_offset": next_offset,
                "offset": cli.offset,
                "returned": returned,
                "total": total,
                "truncated": next_offset.is_some(),
            })
        }
        "members" => json!(index.members(&symbol.id, cli.limit)),
        "callers" => json!(
            index
                .callers(&symbol.id, cli.limit)
                .map_err(|error| error.to_string())?
        ),
        "callees" => json!(
            index
                .callees(&symbol.id, cli.limit)
                .map_err(|error| error.to_string())?
        ),
        "supertypes" => json!(index.supertypes(&symbol.id, cli.limit)),
        "subtypes" => json!(index.subtypes(&symbol.id, cli.limit)),
        _ => unreachable!(),
    };
    emit(&json!({
        "command": cli.command,
        "resolved": symbol.id,
        "result": result,
        "selector": cli.argument,
        "status": "ok",
    }))?;
    Ok(0)
}

fn parse_location(location: &str) -> Result<(String, usize, Option<usize>), String> {
    let (head, last) = location
        .rsplit_once(':')
        .ok_or_else(|| "location must be PATH:LINE[:COLUMN]".to_owned())?;
    let last = positive(last, "line or column")?;
    if let Some((path, line)) = head.rsplit_once(':')
        && let Ok(line) = positive(line, "line")
    {
        if path.is_empty() {
            return Err("location path cannot be empty".to_owned());
        }
        return Ok((path.to_owned(), line, Some(last)));
    }
    if head.is_empty() {
        return Err("location path cannot be empty".to_owned());
    }
    Ok((head.to_owned(), last, None))
}

enum SelectionFailure {
    Ambiguous(Page<SymbolView>),
    NotFound(Page<SymbolView>),
}

fn select(
    index: &QueryIndex,
    selector: &str,
    limit: usize,
) -> Result<SymbolView, SelectionFailure> {
    match index.resolve(selector, None, limit) {
        Resolution::Found { symbol } => Ok(symbol),
        Resolution::Ambiguous { candidates, .. } => Err(SelectionFailure::Ambiguous(candidates)),
        Resolution::NotFound { .. } => {
            let alias = selector
                .rsplit_once('#')
                .filter(|(owner, member)| !owner.is_empty() && !member.is_empty())
                .map(|(owner, member)| format!("{owner}.{member}"));
            if let Some(alias) = alias {
                match index.resolve(&alias, None, limit) {
                    Resolution::Found { symbol } => return Ok(symbol),
                    Resolution::Ambiguous { candidates, .. } => {
                        return Err(SelectionFailure::Ambiguous(candidates));
                    }
                    Resolution::NotFound { .. } => {}
                }
            }
            let needle = selector
                .rsplit(['#', '.', ':', '/'])
                .find(|part| !part.is_empty())
                .unwrap_or(selector)
                .trim_end_matches("()");
            Err(SelectionFailure::NotFound(index.find(needle, limit)))
        }
    }
}

fn emit_selection_failure(
    command: &str,
    selector: &str,
    failure: SelectionFailure,
) -> Result<u8, String> {
    match failure {
        SelectionFailure::Ambiguous(candidates) => emit(&json!({
            "candidates": candidates,
            "command": command,
            "selector": selector,
            "status": "ambiguous",
        }))?,
        SelectionFailure::NotFound(suggestions) => emit(&json!({
            "command": command,
            "selector": selector,
            "status": "not_found",
            "suggestions": suggestions,
        }))?,
    }
    Ok(2)
}

fn reference_document(reference: &ReferenceView) -> Option<&str> {
    match &reference.evidence {
        ReferenceEvidence::Occurrence { occurrence } => Some(&occurrence.document),
        ReferenceEvidence::Relationship { relationship } => relationship.document.as_deref(),
    }
}

fn compact_reference(index: &QueryIndex, reference: &ReferenceView) -> Value {
    let evidence = match &reference.evidence {
        ReferenceEvidence::Occurrence { occurrence } => json!({
            "column": occurrence.range.map(|range| range.start.character + 1),
            "document": occurrence.document,
            "line": occurrence.range.map(|range| range.start.line + 1),
            "roles": occurrence.role_names,
        }),
        ReferenceEvidence::Relationship { relationship } => json!({
            "document": relationship.document,
            "is_definition": relationship.is_definition,
            "is_implementation": relationship.is_implementation,
            "is_reference": relationship.is_reference,
            "is_type_definition": relationship.is_type_definition,
        }),
    };
    json!({
        "evidence": evidence,
        "source": symbol_label(index, &reference.source),
        "source_id": reference.source,
    })
}

fn symbol_label(index: &QueryIndex, id: &SymbolId) -> String {
    match index.resolve(&id.canonical(), None, 1) {
        Resolution::Found { symbol } => symbol.qualified_name,
        _ => id.canonical(),
    }
}

fn emit(value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer_pretty(std::io::stdout().lock(), value)
        .map_err(|error| error.to_string())?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_small_command_surface() {
        let cli = parse(
            [
                "--index",
                "index.scip",
                "--facts",
                "index.tyfacts",
                "references",
                "Thing.run",
                "--path",
                "tests/",
                "--offset",
                "5",
                "--limit",
                "10",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(cli.command, "references");
        assert_eq!(cli.path.as_deref(), Some("tests/"));
        assert_eq!(cli.offset, 5);
        assert_eq!(cli.limit, 10);
    }

    #[test]
    fn rejects_removed_graph_commands() {
        let error = parse(
            ["--index", "index.scip", "affected", "Thing"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )
        .unwrap_err();
        assert_eq!(error, "expected a command, found affected");
    }
}
