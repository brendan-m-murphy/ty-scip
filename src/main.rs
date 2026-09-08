use std::{env, path::PathBuf, process};

mod scip_emit;
mod ty_index;

const USAGE: &str = "Usage: ty-scip [PROJECT_ROOT] [OUTPUT.scip]\n\nIndexes a Python project into index.scip by default.";

fn main() {
    if let Err(error) = run() {
        eprintln!("ty-scip: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let caller_directory = env::current_dir()
        .map_err(|error| format!("cannot determine the current directory: {error}"))?;
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() == 1 && matches!(arguments[0].to_str(), Some("-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    if arguments.len() == 1 && matches!(arguments[0].to_str(), Some("-V" | "--version")) {
        println!("ty-scip {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(option) = arguments
        .iter()
        .find(|argument| argument.to_string_lossy().starts_with('-'))
    {
        return Err(format!(
            "unknown option {}; try `ty-scip --help`",
            option.to_string_lossy()
        ));
    }
    if arguments.len() > 2 {
        return Err(format!("expected at most two arguments\n{USAGE}"));
    }

    let sample_limit = match env::var("TY_SCIP_SAMPLE_LIMIT") {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|_| "TY_SCIP_SAMPLE_LIMIT must be a non-negative integer")?,
        Err(env::VarError::NotPresent) => 0,
        Err(error) => return Err(format!("invalid TY_SCIP_SAMPLE_LIMIT: {error}")),
    };
    let root = arguments.first().map_or_else(
        || Ok(caller_directory.clone()),
        |path| {
            let path = PathBuf::from(path);
            path.canonicalize()
                .map_err(|error| format!("cannot resolve project root {}: {error}", path.display()))
        },
    )?;
    let output = arguments
        .get(1)
        .map_or_else(|| caller_directory.join("index.scip"), PathBuf::from);

    let index = ty_index::index(root, sample_limit)?;
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
