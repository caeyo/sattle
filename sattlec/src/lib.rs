//! sattlec — compiler for the sattle SAT DSL.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use ast::{format_ast, Module};
pub use lexer::{lex, SpannedToken, Token};
pub use parser::{parse, ParseError};
pub use span::{format_tokens, LineIndex};
