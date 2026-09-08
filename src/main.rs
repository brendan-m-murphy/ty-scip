use std::{env, path::PathBuf};

mod scip_emit;
mod ty_index;

fn main() -> Result<(), String> {
    let sample_limit = match env::var("TY_SCIP_SAMPLE_LIMIT") {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|_| "TY_SCIP_SAMPLE_LIMIT must be a non-negative integer")?,
        Err(env::VarError::NotPresent) => 0,
        Err(error) => return Err(format!("invalid TY_SCIP_SAMPLE_LIMIT: {error}")),
    };
    let mut arguments = env::args_os().skip(1);
    let root = arguments
        .next()
        .map_or_else(env::current_dir, |path| PathBuf::from(path).canonicalize());
    let output = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err("usage: ty-scip [PROJECT_ROOT] [OUTPUT.scip]".into());
    }

    let index = ty_index::index(root.map_err(|error| error.to_string())?, sample_limit)?;
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
        let source = &index.files[edge.source_file];
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
        println!(
            "{}:{:?} -> {}:{:?}",
            source.relative_path, edge.source_range, target.relative_path, edge.target_range,
        );
    }
    let definitions = index
        .files
        .iter()
        .map(|file| file.globals.len() + file.locals.len())
        .sum::<usize>();
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

    if let Some(output) = output {
        scip_emit::write_index(&index.root, &output, &index.files, &index.edges)?;
    }
    Ok(())
}
