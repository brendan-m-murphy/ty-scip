use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use protobuf::Message;
use ty_scip::graphify;

const USAGE: &str = "Usage: scip-graphify INPUT.scip [OUTPUT.json]\n\nConvert a SCIP protobuf index into deterministic Graphify JSON. Use '-' as the output path for stdout.";

fn main() {
    if let Err(error) = run() {
        eprintln!("scip-graphify: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let input = match args.next() {
        Some(path) if path == "-h" || path == "--help" => {
            println!("{USAGE}");
            return Ok(());
        }
        Some(path) => PathBuf::from(path),
        None => return Err(USAGE.to_owned()),
    };
    let output = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err(format!("expected one or two paths\n{USAGE}"));
    }
    let bytes =
        fs::read(&input).map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let index = scip::types::Index::parse_from_bytes(&bytes)
        .map_err(|error| format!("cannot decode {}: {error}", input.display()))?;
    let json =
        graphify::to_json(&index).map_err(|error| format!("cannot encode graph: {error}"))?;
    match output {
        None => {
            println!("{json}");
        }
        Some(path) if path.as_os_str() == "-" => {
            println!("{json}");
        }
        Some(path) => {
            if same_file(&input, &path)? {
                return Err(format!(
                    "input and output refer to the same file: {}",
                    path.display()
                ));
            }
            atomic_write(&path, json.as_bytes())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn same_file(input: &Path, output: &Path) -> Result<bool, String> {
    let input = input
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", input.display()))?;
    let output = if output.exists() {
        output
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", output.display()))?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        parent
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", parent.display()))?
            .join(
                output
                    .file_name()
                    .ok_or_else(|| format!("output path has no file name: {}", output.display()))?,
            )
    };
    Ok(input == output)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "output path has no file name")
    })?;
    let mut temporary_name = OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".scip-graphify-{}.tmp", process::id()));
    let temporary = parent.join(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
