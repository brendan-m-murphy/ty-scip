use std::{env, ffi::OsString, path::PathBuf, process};

mod scip_emit;
mod ty_index;

const USAGE: &str = "Usage: ty-scip [OPTIONS] [PROJECT_ROOT] [OUTPUT.scip]\n\nIndexes a Python project into index.scip by default.\n\nOptions:\n  --project-name NAME       Override the SCIP package name\n  --project-version VERSION Override the SCIP package version\n  -h, --help                Print help\n  -V, --version             Print version";

fn main() {
    if let Err(error) = run() {
        eprintln!("ty-scip: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let caller_directory = env::current_dir()
        .map_err(|error| format!("cannot determine the current directory: {error}"))?;
    let mut arguments = env::args_os().skip(1);
    let mut positionals = Vec::new();
    let mut project_name = None;
    let mut project_version = None;
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

    let sample_limit = match env::var("TY_SCIP_SAMPLE_LIMIT") {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|_| "TY_SCIP_SAMPLE_LIMIT must be a non-negative integer")?,
        Err(env::VarError::NotPresent) => 0,
        Err(error) => return Err(format!("invalid TY_SCIP_SAMPLE_LIMIT: {error}")),
    };
    let root = positionals.first().map_or_else(
        || Ok(caller_directory.clone()),
        |path| {
            let path = PathBuf::from(path);
            path.canonicalize()
                .map_err(|error| format!("cannot resolve project root {}: {error}", path.display()))
        },
    )?;
    let output = positionals
        .get(1)
        .map_or_else(|| caller_directory.join("index.scip"), PathBuf::from);

    let index = ty_index::index(root, sample_limit, project_name, project_version)?;
    for sample in &index.samples {
        eprintln!("{sample}");
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
    let definitions = index
        .files
        .iter()
        .map(|file| file.globals.len() + file.locals.len())
        .sum::<usize>();
    scip_emit::write_index(&index.root, &output, &index.files, &index.edges)?;
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

    Ok(())
}

fn option_value(option: &str, value: Option<OsString>) -> Result<String, String> {
    value
        .ok_or_else(|| format!("{option} requires a value"))?
        .into_string()
        .map_err(|_| format!("{option} value must be UTF-8"))
}
