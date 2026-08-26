//! sattlec — compiler for the sattle SAT DSL.

pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod typeck;

pub use ast::{format_ast, Module};
pub use codegen::{emit_llvm_ir, CodegenError};
pub use lexer::{lex, SpannedToken, Token};
pub use parser::{parse, ParseError};
pub use span::{format_tokens, LineIndex};
pub use typeck::{typeck, TypeError};
