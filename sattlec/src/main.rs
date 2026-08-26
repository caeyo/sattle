//! `sattlec` CLI — print tokens for a `.satl` file.

use sattlec::{format_tokens, lex, LineIndex};
use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: sattlec <file.satl>");
        process::exit(2);
    };

    if args.next().is_some() {
        eprintln!("usage: sattlec <file.satl>");
        process::exit(2);
    }

    let path = Path::new(&path);
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(1);
        }
    };

    match lex(&source) {
        Ok(tokens) => print!("{}", format_tokens(&source, &tokens)),
        Err(offset) => {
            let index = LineIndex::new(&source);
            let (line, col) = index.line_col(&source, offset);
            eprintln!(
                "error: unrecognized token at {}:{}:{}",
                path.display(),
                line,
                col
            );
            process::exit(1);
        }
    }
}
