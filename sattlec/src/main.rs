//! `sattlec` CLI — compile / inspect `.satl` sources.

use clap::{ArgGroup, Parser};
use sattlec::{
    emit_llvm_ir, format_ast, format_tokens, lex, parse, typeck, LineIndex,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Parser)]
#[command(name = "sattlec", about = "Compiler for the sattle SAT DSL")]
#[command(group(ArgGroup::new("dump").args(["tokens", "ast", "emit_llvm"])))]
struct Cli {
    /// Dump tokens
    #[arg(long)]
    tokens: bool,

    /// Dump the AST
    #[arg(long)]
    ast: bool,

    /// Emit LLVM IR (default)
    #[arg(long = "emit-llvm")]
    emit_llvm: bool,

    /// Input `.satl` source file
    input: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DumpMode {
    Tokens,
    Ast,
    EmitLlvm,
}

impl Cli {
    fn dump_mode(&self) -> DumpMode {
        if self.tokens {
            DumpMode::Tokens
        } else if self.ast {
            DumpMode::Ast
        } else {
            DumpMode::EmitLlvm
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
            report_error(&source, path, Some(offset), "unrecognized token");
            process::exit(1);
        }
    };

    let mode = cli.dump_mode();
    if mode == DumpMode::Tokens {
        print!("{}", format_tokens(&source, &tokens));
        return;
    }

    let module = match parse(&tokens, source.len()) {
        Ok(module) => module,
        Err(err) => {
            report_error(&source, path, Some(err.offset), &err.message);
            process::exit(1);
        }
    };

    if mode == DumpMode::Ast {
        print!("{}", format_ast(&module));
        return;
    }

    if let Err(err) = typeck(&module) {
        report_error(&source, path, None, &err.message);
        process::exit(1);
    }
    match emit_llvm_ir(&module, &path.display().to_string()) {
        Ok(ir) => print!("{ir}"),
        Err(err) => {
            report_error(&source, path, None, &err.message);
            process::exit(1);
        }
    }
}

fn report_error(source: &str, path: &Path, offset: Option<usize>, message: &str) {
    match offset {
        Some(offset) => {
            let index = LineIndex::new(source);
            let (line, col) = index.line_col(source, offset);
            eprintln!(
                "error: {message} at {}:{}:{}",
                path.display(),
                line,
                col
            );
        }
        None => {
            eprintln!("error: {message} ({})", path.display());
        }
    }
}
