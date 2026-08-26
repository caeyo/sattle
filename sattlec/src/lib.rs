//! sattlec — compiler for the sattle SAT DSL.

pub mod lexer;
pub mod span;

pub use lexer::{lex, SpannedToken, Token};
pub use span::{format_tokens, LineIndex};
