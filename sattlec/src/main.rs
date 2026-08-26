//! `sattlec` CLI — compile / inspect `.satl` sources.

use clap::{ArgGroup, Parser};
use sattlec::{format_ast, format_tokens, lex, parse, LineIndex};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Parser)]
#[command(name = "sattlec", about = "Compiler for the sattle SAT DSL")]
#[command(group(ArgGroup::new("dump").args(["tokens", "ast"])))]
struct Cli {
    /// Dump tokens
    #[arg(long)]
    tokens: bool,

    /// Dump the AST (default)
    #[arg(long)]
    ast: bool,

    /// Input `.satl` source file
    input: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DumpMode {
    Tokens,
    Ast,
}

impl Cli {
    fn dump_mode(&self) -> DumpMode {
        if self.tokens {
            DumpMode::Tokens
        } else {
            DumpMode::Ast
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let path = &cli.input;

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(1);
        }
    };

    let tokens = match lex(&source) {
        Ok(tokens) => tokens,
        Err(offset) => {
            report_error(&source, path, offset, "unrecognized token");
            process::exit(1);
        }
    };

    match cli.dump_mode() {
        DumpMode::Tokens => {
            print!("{}", format_tokens(&source, &tokens));
        }
        DumpMode::Ast => match parse(&tokens, source.len()) {
            Ok(module) => print!("{}", format_ast(&module)),
            Err(err) => {
                report_error(&source, path, err.offset, &err.message);
                process::exit(1);
            }
        },
    }
}

fn report_error(source: &str, path: &Path, offset: usize, message: &str) {
    let index = LineIndex::new(source);
    let (line, col) = index.line_col(source, offset);
    eprintln!(
        "error: {message} at {}:{}:{}",
        path.display(),
        line,
        col
    );
}
