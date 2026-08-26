//! `sattlec` CLI — compile / inspect `.satl` sources.

use clap::{ArgGroup, Parser};
use sattlec::{
    compile_executable, emit_llvm_ir, format_ast, format_tokens, lex, parse, typeck, LineIndex,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Parser)]
#[command(name = "sattlec", about = "Compiler for the sattle SAT DSL")]
#[command(group(ArgGroup::new("action").args(["tokens", "ast", "emit_llvm", "output"])))]
struct Cli {
    /// Dump tokens
    #[arg(long)]
    tokens: bool,

    /// Dump the AST
    #[arg(long)]
    ast: bool,

    /// Emit LLVM IR
    #[arg(long = "emit-llvm")]
    emit_llvm: bool,

    /// Write a native executable (default: a.out)
    #[arg(short = 'o', value_name = "FILE")]
    output: Option<PathBuf>,

    /// Input `.satl` source file
    input: PathBuf,
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

    if cli.tokens {
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

    if cli.ast {
        print!("{}", format_ast(&module));
        return;
    }

    if let Err(err) = typeck(&module) {
        report_error(&source, path, None, &err.message);
        process::exit(1);
    }

    let source_name = path.display().to_string();
    if cli.emit_llvm {
        match emit_llvm_ir(&module, &source_name) {
            Ok(ir) => print!("{ir}"),
            Err(err) => {
                report_error(&source, path, None, &err.message);
                process::exit(1);
            }
        }
        return;
    }

    let default_output = PathBuf::from("a.out");
    let output = cli.output.as_ref().unwrap_or(&default_output);
    if let Err(err) = compile_executable(&module, &source_name, output) {
        report_error(&source, path, None, &err.message);
        process::exit(1);
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
