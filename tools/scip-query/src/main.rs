use std::{env, io, path::PathBuf, process::ExitCode};

use scip_query::{
    Page, QueryIndex, RefDirection, ReferenceEvidence, ReferenceView, Resolution, SqlDatabase,
    SqlSelectionError, SymbolId, SymbolView,
};
use serde::Serialize;
use serde_json::json;

const DEFAULT_LIMIT: usize = 50;
const DEFAULT_DEPTH: usize = 4;
const USAGE: &str = r#"Usage:
  scip-query --index INDEX.scip [--root PATH] [--limit N] COMMAND ...
  scip-query [--root PATH] [--limit N] INDEX.scip COMMAND ...
  scip-query sql-refs DATABASE SELECTOR [--incoming|--outgoing|--both]
             [--path PREFIX] [--offset N] [--limit N]
  scip-query sql-tests DATABASE SELECTOR [--path PREFIX] [--depth N]
             [--offset N] [--limit N]
  scip-query sql-stats DATABASE

Commands:
  find QUERY [--path PATH] [--limit N]
  at PATH:LINE[:COLUMN] [--limit N]
  context SELECTOR
  refs SELECTOR [--incoming|--outgoing|--both] [--path PREFIX] [--compact]
       [--offset N] [--limit N]
  members SELECTOR [--limit N]
  path SOURCE TARGET [--max-depth N|--depth N] [--limit N]
  affected SELECTOR [--max-depth N|--depth N] [--limit N]
  build-db DATABASE
  sql-refs DATABASE SELECTOR [--incoming|--outgoing|--both] [--path PREFIX]
           [--offset N] [--limit N]
  sql-tests DATABASE SELECTOR [--path PREFIX] [--depth N] [--offset N] [--limit N]
  sql-stats DATABASE

Selectors accept raw SCIP symbols and path-qualified names."#;

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    index: Option<PathBuf>,
    root: Option<PathBuf>,
    command: Command,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Find {
        query: String,
        path: Option<String>,
        limit: usize,
    },
    At {
        path: String,
        line: usize,
        column: Option<usize>,
        limit: usize,
    },
    Context {
        selector: String,
        limit: usize,
    },
    Refs {
        selector: String,
        direction: Direction,
        path: Option<String>,
        compact: bool,
        offset: usize,
        limit: usize,
    },
    Members {
        selector: String,
        limit: usize,
    },
    Path {
        source: String,
        target: String,
        depth: usize,
        limit: usize,
    },
    Affected {
        selector: String,
        depth: usize,
        limit: usize,
    },
    BuildDb {
        database: PathBuf,
    },
    SqlRefs {
        database: PathBuf,
        selector: String,
        direction: Direction,
        path: Option<String>,
        offset: usize,
        limit: usize,
    },
    SqlStats {
        database: PathBuf,
    },
    SqlTests {
        database: PathBuf,
        selector: String,
        path: String,
        depth: usize,
        offset: usize,
        limit: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Incoming,
    Outgoing,
    Both,
}

enum ParseResult {
    Help,
    Run(Cli),
}

fn main() -> ExitCode {
    match parse(env::args().skip(1).collect()) {
        Ok(ParseResult::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParseResult::Run(cli)) => match execute(cli) {
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

fn parse(args: Vec<String>) -> Result<ParseResult, String> {
    if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(ParseResult::Help);
    }

    let mut index = None;
    let mut root = None;
    let mut limit = DEFAULT_LIMIT;
    let mut position = 0;
    while position < args.len() && !is_command(&args[position]) {
        match args[position].as_str() {
            "--index" => index = Some(PathBuf::from(value(&args, &mut position, "--index")?)),
            "--root" => root = Some(PathBuf::from(value(&args, &mut position, "--root")?)),
            "--limit" => limit = positive(&value(&args, &mut position, "--limit")?, "limit")?,
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            path if index.is_none() => index = Some(PathBuf::from(path)),
            argument => return Err(format!("expected a command, found {argument}")),
        }
        position += 1;
    }
    let name = args
        .get(position)
        .ok_or_else(|| "missing command".to_owned())?;
    let command = parse_command(name, &args[position + 1..], limit)?;
    if index.is_none()
        && !matches!(
            command,
            Command::SqlRefs { .. } | Command::SqlStats { .. } | Command::SqlTests { .. }
        )
    {
        return Err("missing SCIP index (--index INDEX.scip)".to_owned());
    }
    Ok(ParseResult::Run(Cli {
        index,
        root,
        command,
    }))
}

fn parse_command(name: &str, args: &[String], default_limit: usize) -> Result<Command, String> {
    let mut positional = Vec::new();
    let mut path = None;
    let mut limit = default_limit;
    let mut depth = DEFAULT_DEPTH;
    let mut direction = Direction::Both;
    let mut direction_seen = false;
    let mut compact = false;
    let mut offset = 0;
    let mut position = 0;
    while position < args.len() {
        match args[position].as_str() {
            "--limit" => limit = positive(&value(args, &mut position, "--limit")?, "limit")?,
            "--path" if matches!(name, "find" | "refs" | "sql-refs" | "sql-tests") => {
                path = Some(value(args, &mut position, "--path")?)
            }
            "--compact" if name == "refs" => compact = true,
            "--offset" if matches!(name, "refs" | "sql-refs" | "sql-tests") => {
                offset = value(args, &mut position, "--offset")?
                    .parse::<usize>()
                    .map_err(|_| "offset must be a non-negative integer".to_owned())?;
            }
            "--depth" | "--max-depth"
                if name == "path" || name == "affected" || name == "sql-tests" =>
            {
                let option = args[position].clone();
                depth = positive(&value(args, &mut position, &option)?, "depth")?;
            }
            "--incoming" | "--outgoing" | "--both" if name == "refs" || name == "sql-refs" => {
                if direction_seen {
                    return Err("choose only one refs direction".to_owned());
                }
                direction_seen = true;
                direction = match args[position].as_str() {
                    "--incoming" => Direction::Incoming,
                    "--outgoing" => Direction::Outgoing,
                    _ => Direction::Both,
                };
            }
            option if option.starts_with('-') => {
                return Err(format!("unknown option {option} for {name}"));
            }
            argument => positional.push(argument.to_owned()),
        }
        position += 1;
    }

    match (name, positional.as_slice()) {
        ("find", [query]) => Ok(Command::Find {
            query: query.clone(),
            path,
            limit,
        }),
        ("at", [location]) => {
            let (path, line, column) = parse_location(location)?;
            Ok(Command::At {
                path,
                line,
                column,
                limit,
            })
        }
        ("context", [selector]) => Ok(Command::Context {
            selector: selector.clone(),
            limit,
        }),
        ("refs", [selector]) => Ok(Command::Refs {
            selector: selector.clone(),
            direction,
            path,
            compact,
            offset,
            limit,
        }),
        ("members", [selector]) => Ok(Command::Members {
            selector: selector.clone(),
            limit,
        }),
        ("path", [source, target]) => Ok(Command::Path {
            source: source.clone(),
            target: target.clone(),
            depth,
            limit,
        }),
        ("affected", [selector]) => Ok(Command::Affected {
            selector: selector.clone(),
            depth,
            limit,
        }),
        ("build-db", [database]) => Ok(Command::BuildDb {
            database: PathBuf::from(database),
        }),
        ("sql-refs", [database, selector]) => Ok(Command::SqlRefs {
            database: PathBuf::from(database),
            selector: selector.clone(),
            direction,
            path,
            offset,
            limit,
        }),
        ("sql-stats", [database]) => Ok(Command::SqlStats {
            database: PathBuf::from(database),
        }),
        ("sql-tests", [database, selector]) => Ok(Command::SqlTests {
            database: PathBuf::from(database),
            selector: selector.clone(),
            path: path.unwrap_or_else(|| "tests/".to_owned()),
            depth,
            offset,
            limit,
        }),
        (known, _) if is_command(known) => Err(format!("wrong number of arguments for {known}")),
        _ => Err(format!("unknown command {name}")),
    }
}

fn is_command(value: &str) -> bool {
    matches!(
        value,
        "find"
            | "at"
            | "context"
            | "refs"
            | "members"
            | "path"
            | "affected"
            | "build-db"
            | "sql-refs"
            | "sql-tests"
            | "sql-stats"
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

fn execute(cli: Cli) -> Result<u8, String> {
    if let Command::SqlRefs {
        database,
        selector,
        direction,
        path,
        offset,
        limit,
    } = &cli.command
    {
        let database = SqlDatabase::open(database).map_err(|error| error.to_string())?;
        let (direction_name, direction) = ref_direction(*direction);
        let (resolved, result) =
            match database.refs(selector, direction, path.as_deref(), *offset, *limit) {
                Ok(result) => result,
                Err(error) => return emit_sql_selection_failure("sql-refs", selector, error),
            };
        emit(&json!({
            "command": "sql-refs",
            "database": database_path(&cli.command),
            "direction": direction_name,
            "path": path,
            "resolved": resolved,
            "result": result,
            "selector": selector,
            "status": "ok",
        }))?;
        return Ok(0);
    }
    if let Command::SqlTests {
        database,
        selector,
        path,
        depth,
        offset,
        limit,
    } = &cli.command
    {
        let sql = SqlDatabase::open(database).map_err(|error| error.to_string())?;
        let (resolved, result) = match sql.tests(selector, path, *depth, *offset, *limit) {
            Ok(result) => result,
            Err(error) => return emit_sql_selection_failure("sql-tests", selector, error),
        };
        emit(&json!({
            "command": "sql-tests",
            "database": database,
            "path": path,
            "max_depth": depth,
            "resolved": resolved,
            "result": result,
            "selector": selector,
            "status": "ok",
        }))?;
        return Ok(0);
    }
    if let Command::SqlStats { database } = &cli.command {
        let result = SqlDatabase::open(database)
            .and_then(|database| database.stats())
            .map_err(|error| error.to_string())?;
        emit(&json!({
            "command": "sql-stats",
            "database": database,
            "result": result,
            "status": "ok",
        }))?;
        return Ok(0);
    }
    let index_path = cli
        .index
        .as_ref()
        .ok_or_else(|| "missing SCIP index (--index INDEX.scip)".to_owned())?;
    let index = QueryIndex::load(index_path, cli.root).map_err(|error| error.to_string())?;
    match cli.command {
        Command::Find { query, path, limit } => {
            let result = index.find_in(&query, path.as_deref(), limit);
            emit(&json!({
                "command": "find",
                "path": path,
                "query": query,
                "result": result,
                "status": "ok",
            }))?;
        }
        Command::At {
            path,
            line,
            column,
            limit,
        } => {
            let result = index
                .at(&path, line, column, limit)
                .map_err(|error| error.to_string())?;
            emit(&json!({
                "command": "at",
                "location": { "column": column, "line": line, "path": path },
                "result": result,
                "status": "ok",
            }))?;
        }
        Command::Context { selector, limit } => {
            let symbol = match select(&index, &selector, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => {
                    return emit_selection_failure("context", "selector", &selector, failure);
                }
            };
            let result = index
                .context_with_limit(&symbol.id, 3, 3, limit)
                .map_err(|error| error.to_string())?;
            emit(&json!({
                "command": "context",
                "resolved": symbol.id,
                "result": result,
                "selector": selector,
                "status": "ok",
            }))?;
        }
        Command::Refs {
            selector,
            direction,
            path,
            compact,
            offset,
            limit,
        } => {
            let symbol = match select(&index, &selector, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => {
                    return emit_selection_failure("refs", "selector", &selector, failure);
                }
            };
            let (direction_name, direction) = match direction {
                Direction::Incoming => ("incoming", RefDirection::Incoming),
                Direction::Outgoing => ("outgoing", RefDirection::Outgoing),
                Direction::Both => ("both", RefDirection::Both),
            };
            let mut items = index.refs(&symbol.id, direction, usize::MAX).items;
            if let Some(prefix) = path.as_deref() {
                items.retain(|reference| {
                    reference_document(reference)
                        .is_some_and(|document| document.starts_with(prefix))
                });
            }
            let total = items.len();
            let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
            let returned = items.len();
            let next_offset = (offset + returned < total).then_some(offset + returned);
            let items = if compact {
                items
                    .iter()
                    .map(|reference| compact_reference(&index, reference))
                    .collect::<Vec<_>>()
            } else {
                items.iter().map(|reference| json!(reference)).collect()
            };
            emit(&json!({
                "compact": compact,
                "command": "refs",
                "direction": direction_name,
                "path": path,
                "resolved": symbol.id,
                "result": {
                    "items": items,
                    "next_offset": next_offset,
                    "offset": offset,
                    "returned": returned,
                    "total": total,
                    "truncated": next_offset.is_some(),
                },
                "selector": selector,
                "status": "ok",
            }))?;
        }
        Command::Members { selector, limit } => {
            let symbol = match select(&index, &selector, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => {
                    return emit_selection_failure("members", "selector", &selector, failure);
                }
            };
            let result = index.members(&symbol.id, limit);
            emit(&json!({
                "command": "members",
                "resolved": symbol.id,
                "result": result,
                "selector": selector,
                "status": "ok",
            }))?;
        }
        Command::Path {
            source,
            target,
            depth,
            limit,
        } => {
            let source_symbol = match select(&index, &source, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => return emit_selection_failure("path", "source", &source, failure),
            };
            let target_symbol = match select(&index, &target, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => return emit_selection_failure("path", "target", &target, failure),
            };
            let result = index.path(&source_symbol.id, &target_symbol.id, depth);
            emit(&json!({
                "command": "path",
                "result": result,
                "source": { "resolved": source_symbol.id, "selector": source },
                "status": "ok",
                "target": { "resolved": target_symbol.id, "selector": target },
            }))?;
        }
        Command::Affected {
            selector,
            depth,
            limit,
        } => {
            let symbol = match select(&index, &selector, None, limit) {
                Ok(symbol) => symbol,
                Err(failure) => {
                    return emit_selection_failure("affected", "selector", &selector, failure);
                }
            };
            let result = index.affected(&symbol.id, depth, limit);
            emit(&json!({
                "command": "affected",
                "max_depth": depth,
                "resolved": symbol.id,
                "result": result,
                "selector": selector,
                "status": "ok",
            }))?;
        }
        Command::BuildDb { database } => {
            let result = index
                .write_database(&database)
                .map_err(|error| error.to_string())?;
            emit(&json!({
                "command": "build-db",
                "database": database,
                "result": result,
                "status": "ok",
            }))?;
        }
        Command::SqlRefs { .. } | Command::SqlStats { .. } | Command::SqlTests { .. } => {
            unreachable!()
        }
    }
    Ok(0)
}

fn ref_direction(direction: Direction) -> (&'static str, RefDirection) {
    match direction {
        Direction::Incoming => ("incoming", RefDirection::Incoming),
        Direction::Outgoing => ("outgoing", RefDirection::Outgoing),
        Direction::Both => ("both", RefDirection::Both),
    }
}

fn database_path(command: &Command) -> Option<&PathBuf> {
    match command {
        Command::SqlRefs { database, .. }
        | Command::SqlStats { database }
        | Command::SqlTests { database, .. } => Some(database),
        _ => None,
    }
}

fn emit_sql_selection_failure(
    command: &str,
    selector: &str,
    error: Box<dyn std::error::Error + Send + Sync>,
) -> Result<u8, String> {
    let Some(selection) = error.downcast_ref::<SqlSelectionError>() else {
        return Err(error.to_string());
    };
    match selection {
        SqlSelectionError::Ambiguous { candidates, .. } => emit(&json!({
            "candidates": candidates,
            "command": command,
            "selector": selector,
            "status": "ambiguous",
        }))?,
        SqlSelectionError::NotFound { .. } => emit(&json!({
            "command": command,
            "selector": selector,
            "status": "not_found",
        }))?,
    }
    Ok(2)
}

enum SelectionFailure {
    Ambiguous(Page<SymbolView>),
    NotFound(Page<SymbolView>),
}

fn select(
    index: &QueryIndex,
    selector: &str,
    document: Option<&str>,
    limit: usize,
) -> Result<SymbolView, SelectionFailure> {
    match index.resolve(selector, document, limit) {
        Resolution::Found { symbol } => Ok(symbol),
        Resolution::Ambiguous { candidates, .. } => Err(SelectionFailure::Ambiguous(candidates)),
        Resolution::NotFound { .. } => {
            if let Some(alias) = hash_selector_alias(selector) {
                match index.resolve(&alias, document, limit) {
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

fn hash_selector_alias(selector: &str) -> Option<String> {
    if selector.contains(' ') {
        return None;
    }
    let (owner, member) = selector.rsplit_once('#')?;
    (!owner.is_empty() && !member.is_empty()).then(|| format!("{owner}.{member}"))
}

fn reference_document(reference: &ReferenceView) -> Option<&str> {
    match &reference.evidence {
        ReferenceEvidence::Occurrence { occurrence } => Some(&occurrence.document),
        ReferenceEvidence::Relationship { relationship } => relationship.document.as_deref(),
    }
}

fn symbol_label(index: &QueryIndex, id: &SymbolId) -> String {
    match index.resolve(&id.canonical(), None, 1) {
        Resolution::Found { symbol } => symbol.qualified_name,
        _ => id.canonical(),
    }
}

fn compact_reference(index: &QueryIndex, reference: &ReferenceView) -> serde_json::Value {
    let evidence = match &reference.evidence {
        ReferenceEvidence::Occurrence { occurrence } => json!({
            "column": occurrence.range.map(|range| range.start.character + 1),
            "document": occurrence.document,
            "line": occurrence.range.map(|range| range.start.line + 1),
            "provenance": occurrence.provenance,
            "roles": occurrence.role_names,
        }),
        ReferenceEvidence::Relationship { relationship } => json!({
            "document": relationship.document,
            "is_definition": relationship.is_definition,
            "is_implementation": relationship.is_implementation,
            "is_reference": relationship.is_reference,
            "is_type_definition": relationship.is_type_definition,
            "provenance": relationship.provenance,
        }),
    };
    json!({
        "evidence": evidence,
        "source": symbol_label(index, &reference.source),
        "target": symbol_label(index, &reference.target),
    })
}

fn emit_selection_failure(
    command: &str,
    role: &str,
    selector: &str,
    failure: SelectionFailure,
) -> Result<u8, String> {
    match failure {
        SelectionFailure::Ambiguous(candidates) => emit(&json!({
            "candidates": candidates,
            "command": command,
            "selector": selector,
            "selector_role": role,
            "status": "ambiguous",
        }))?,
        SelectionFailure::NotFound(suggestions) => emit(&json!({
            "command": command,
            "selector": selector,
            "selector_role": role,
            "status": "not_found",
            "suggestions": suggestions,
        }))?,
    }
    Ok(2)
}

fn emit(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|error| format!("cannot encode JSON: {error}"))?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_index_and_command_options() {
        let ParseResult::Run(cli) = parse(vec![
            "--index".into(),
            "index.scip".into(),
            "--root".into(),
            ".".into(),
            "refs".into(),
            "pkg/a.py:A.f".into(),
            "--outgoing".into(),
            "--limit".into(),
            "12".into(),
        ])
        .expect("parse") else {
            panic!("expected runnable command");
        };
        assert_eq!(cli.index, Some(PathBuf::from("index.scip")));
        assert_eq!(cli.root, Some(PathBuf::from(".")));
        assert_eq!(
            cli.command,
            Command::Refs {
                selector: "pkg/a.py:A.f".into(),
                direction: Direction::Outgoing,
                path: None,
                compact: false,
                offset: 0,
                limit: 12,
            }
        );
    }

    #[test]
    fn parses_positional_index_and_location() {
        let ParseResult::Run(cli) = parse(vec![
            "index.scip".into(),
            "at".into(),
            "pkg/a.py:7:3".into(),
        ])
        .expect("parse") else {
            panic!("expected runnable command");
        };
        assert_eq!(
            cli.command,
            Command::At {
                path: "pkg/a.py".into(),
                line: 7,
                column: Some(3),
                limit: DEFAULT_LIMIT,
            }
        );
    }

    #[test]
    fn keeps_line_only_location_distinct_from_exact_position() {
        let ParseResult::Run(cli) =
            parse(vec!["index.scip".into(), "at".into(), "pkg/a.py:7".into()]).expect("parse")
        else {
            panic!("expected runnable command");
        };
        assert_eq!(
            cli.command,
            Command::At {
                path: "pkg/a.py".into(),
                line: 7,
                column: None,
                limit: DEFAULT_LIMIT,
            }
        );
    }

    #[test]
    fn rejects_conflicting_reference_directions() {
        let error = parse(vec![
            "index.scip".into(),
            "refs".into(),
            "symbol".into(),
            "--incoming".into(),
            "--outgoing".into(),
        ])
        .err()
        .expect("error");
        assert!(error.contains("only one"));
    }

    #[test]
    fn parses_sql_query_without_scip_index() {
        let ParseResult::Run(cli) = parse(vec![
            "sql-refs".into(),
            "cache.sqlite".into(),
            "pkg.Alpha#run".into(),
            "--outgoing".into(),
            "--path".into(),
            "tests/".into(),
            "--offset".into(),
            "2".into(),
        ])
        .expect("parse") else {
            panic!("expected runnable command");
        };
        assert_eq!(cli.index, None);
        assert_eq!(
            cli.command,
            Command::SqlRefs {
                database: PathBuf::from("cache.sqlite"),
                selector: "pkg.Alpha#run".into(),
                direction: Direction::Outgoing,
                path: Some("tests/".into()),
                offset: 2,
                limit: DEFAULT_LIMIT,
            }
        );
    }

    #[test]
    fn parses_sql_tests_with_default_test_path() {
        let ParseResult::Run(cli) = parse(vec![
            "sql-tests".into(),
            "cache.sqlite".into(),
            "BaseStore".into(),
        ])
        .expect("parse") else {
            panic!("expected runnable command");
        };
        assert_eq!(cli.index, None);
        assert_eq!(
            cli.command,
            Command::SqlTests {
                database: PathBuf::from("cache.sqlite"),
                selector: "BaseStore".into(),
                path: "tests/".into(),
                depth: DEFAULT_DEPTH,
                offset: 0,
                limit: DEFAULT_LIMIT,
            }
        );
    }
}
