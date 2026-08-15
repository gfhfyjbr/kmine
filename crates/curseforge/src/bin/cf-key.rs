use kmine_curseforge::{extract_from_source, extract_from_bytes, CfKeyError, LATEST_MAC_DMG};
use std::env;
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = args.next();
    if matches!(path.as_deref(), Some("-h" | "--help")) {
        eprintln!("usage: cf-key [asar | .app | zip | dmg | url]");
        eprintln!("       cf-key                   # fetch {LATEST_MAC_DMG}");
        eprintln!("       cf-key -                 # read bytes from stdin");
        return ExitCode::SUCCESS;
    }
    let found = if path.as_deref() == Some("-") {
        let mut buf = Vec::new();
        if let Err(err) = io::stdin().read_to_end(&mut buf) {
            eprintln!("stdin: {err}");
            return ExitCode::FAILURE;
        }
        match extract_from_bytes(&buf) {
            Some(found) => found,
            None => {
                eprintln!("no CurseForge Core API key found on stdin");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let source = path.as_deref().unwrap_or(LATEST_MAC_DMG);
        match extract_from_source(source) {
            Ok(found) => found,
            Err(CfKeyError::NotFound(where_)) => {
                eprintln!("no CurseForge Core API key found in {where_}");
                return ExitCode::FAILURE;
            }
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::FAILURE;
            }
        }
    };
    eprintln!("source: {}", found.source);
    println!("{}", found.key);
    ExitCode::SUCCESS
}
