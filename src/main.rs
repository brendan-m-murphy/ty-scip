use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    process,
};

mod scip_emit;
mod ty_index;

const USAGE: &str = "Usage: ty-scip [index] [OPTIONS] [PROJECT_PATH] [OUTPUT.scip]\n\nIndexes a Python project into index.scip by default.\n\nOptions:\n  --output PATH              Write the index to PATH\n  --facts PATH               Write synchronized ty-specific facts to PATH\n  --cwd PATH                 Resolve relative paths from PATH\n  --quiet                    Suppress indexing diagnostics\n  --project-name NAME        Override the SCIP package name\n  --project-version VERSION  Override the SCIP package version\n  -h, --help                 Print help\n  -V, --version              Print version";

fn main() {
    if let Err(error) = run() {
        eprintln!("ty-scip: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let caller_directory = env::current_dir()
        .map_err(|error| format!("cannot determine the current directory: {error}"))?;
    let mut arguments = env::args_os().skip(1).peekable();
    if arguments
        .peek()
        .is_some_and(|argument| argument == OsStr::new("index"))
    {
        arguments.next();
    }
    let mut positionals = Vec::new();
    let mut project_name = None;
    let mut project_version = None;
    let mut output_option = None;
    let mut facts_option = None;
    let mut cwd = None;
    let mut quiet = false;
    let mut options = true;
    while let Some(argument) = arguments.next() {
        let text = argument.to_str();
        if options {
            match text {
                Some("--") => {
                    options = false;
                    continue;
                }
                Some("-h" | "--help") => {
                    println!("{USAGE}");
                    return Ok(());
                }
                Some("-V" | "--version") => {
                    println!("ty-scip {}", env!("CARGO_PKG_VERSION"));
                    return Ok(());
                }
                Some("--output") => {
                    output_option = Some(path_option_value("--output", arguments.next())?);
                    continue;
                }
                Some("--facts") => {
                    facts_option = Some(path_option_value("--facts", arguments.next())?);
                    continue;
                }
                Some("--cwd") => {
                    cwd = Some(path_option_value("--cwd", arguments.next())?);
                    continue;
                }
                Some("--quiet") => {
                    quiet = true;
                    continue;
                }
                Some("--project-name") => {
                    project_name = Some(option_value("--project-name", arguments.next())?);
                    continue;
                }
                Some("--project-version") => {
                    project_version = Some(option_value("--project-version", arguments.next())?);
                    continue;
                }
                Some(value) if value.starts_with("--project-name=") => {
                    project_name = Some(value["--project-name=".len()..].to_owned());
                    continue;
                }
                Some(value) if value.starts_with("--project-version=") => {
                    project_version = Some(value["--project-version=".len()..].to_owned());
                    continue;
                }
                Some(value) if value.starts_with("--output=") => {
                    output_option = Some(PathBuf::from(&value["--output=".len()..]));
                    continue;
                }
                Some(value) if value.starts_with("--facts=") => {
                    facts_option = Some(PathBuf::from(&value["--facts=".len()..]));
                    continue;
                }
                Some(value) if value.starts_with("--cwd=") => {
                    cwd = Some(PathBuf::from(&value["--cwd=".len()..]));
                    continue;
                }
                Some(value) if value.starts_with('-') => {
                    return Err(format!("unknown option {value}; try `ty-scip --help`"));
                }
                _ => {}
            }
        }
        positionals.push(argument);
    }
    if positionals.len() > 2 {
        return Err(format!("expected at most two arguments\n{USAGE}"));
    }
    if output_option.is_some() && positionals.get(1).is_some() {
        return Err("cannot use positional OUTPUT.scip with --output".to_owned());
    }

    let working_directory = cwd.map_or_else(
        || Ok(caller_directory.clone()),
        |path| {
            let path = if path.is_absolute() {
                path
            } else {
                caller_directory.join(path)
            };
            path.canonicalize()
                .map_err(|error| format!("cannot resolve --cwd {}: {error}", path.display()))
        },
    )?;

    let sample_limit = match env::var("TY_SCIP_SAMPLE_LIMIT") {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|_| "TY_SCIP_SAMPLE_LIMIT must be a non-negative integer")?,
        Err(env::VarError::NotPresent) => 0,
        Err(error) => return Err(format!("invalid TY_SCIP_SAMPLE_LIMIT: {error}")),
    };
    let root = positionals.first().map_or_else(
        || Ok(working_directory.clone()),
        |path| {
            let path = PathBuf::from(path);
            let path = if path.is_absolute() {
                path
            } else {
                working_directory.join(path)
            };
            path.canonicalize()
                .map_err(|error| format!("cannot resolve project path {}: {error}", path.display()))
        },
    )?;
    let output = output_option
        .or_else(|| positionals.get(1).map(PathBuf::from))
        .map_or_else(
            || working_directory.join("index.scip"),
            |path| {
                if path.is_absolute() {
                    path
                } else {
                    working_directory.join(path)
                }
            },
        );
    let facts_output = facts_option.map(|path| {
        if path.is_absolute() {
            path
        } else {
            working_directory.join(path)
        }
    });
    if facts_output.as_ref() == Some(&output) {
        return Err("--facts must not overwrite the SCIP index".to_owned());
    }

    let index = ty_index::index(
        root,
        sample_limit,
        project_name,
        project_version,
        facts_output.is_some(),
    )?;
    if !quiet {
        for sample in &index.samples {
            eprintln!("{sample}");
        }
    }

    let mut references = 0;
    let mut skipped_cross_file_locals = 0;
    let mut skipped_missing_symbols = 0;
    for edge in &index.edges {
        if edge.source_file == edge.target_file && edge.source_range == edge.target_range {
            continue;
        }
        let target = &index.files[edge.target_file];
        let Some(symbol) = target
            .globals
            .get(&edge.target_range)
            .or_else(|| target.locals.get(&edge.target_range))
        else {
            skipped_missing_symbols += 1;
            continue;
        };
        if edge.source_file != edge.target_file && symbol.is_local() {
            skipped_cross_file_locals += 1;
            continue;
        }
        references += 1;
    }
    references += index
        .files
        .iter()
        .map(|file| file.external_references.len())
        .sum::<usize>();
    let definitions = index
        .files
        .iter()
        .map(|file| file.globals.len() + file.locals.len())
        .sum::<usize>();
    scip_emit::write_index(
        &index.root,
        &output,
        facts_output.as_deref(),
        &index.files,
        &index.edges,
        &index.ide_symbols,
    )
    .map_err(|error| format!("cannot write SCIP index {}: {error}", output.display()))?;
    if !quiet {
        eprintln!(
            "indexed {} files: {definitions} definitions, {references} references; \
             {} unresolved, {} ambiguous, {} external, {} skipped \
             ({skipped_cross_file_locals} cross-file local, {skipped_missing_symbols} missing symbol)",
            index.files.len(),
            index.unresolved,
            index.ambiguous,
            index.external,
            skipped_cross_file_locals + skipped_missing_symbols,
        );
        if index.syntax_errors != 0 || index.unsupported_syntax_errors != 0 {
            eprintln!(
                "{} syntax errors, {} unsupported syntax errors",
                index.syntax_errors, index.unsupported_syntax_errors
            );
        }
    }

    Ok(())
}

fn path_option_value(option: &str, value: Option<OsString>) -> Result<PathBuf, String> {
    value
        .map(PathBuf::from)
        .ok_or_else(|| format!("{option} requires a value"))
}

fn option_value(option: &str, value: Option<OsString>) -> Result<String, String> {
    value
        .ok_or_else(|| format!("{option} requires a value"))?
        .into_string()
        .map_err(|_| format!("{option} value must be UTF-8"))
}
