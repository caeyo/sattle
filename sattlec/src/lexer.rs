//! Lexer for sattle (`.satl`) source.

use logos::Logos;
use std::fmt;

/// A lexeme with its byte span in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken<'a> {
    pub kind: Token<'a>,
    pub span: std::ops::Range<usize>,
}

/// Token kinds produced by the lexer.
#[derive(Logos, Debug, Clone, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token<'a> {
    // --- keywords (general) ---
    #[token("fn")]
    Fn,
    #[token("struct")]
    Struct,
    #[token("enum")]
    Enum,
    #[token("impl")]
    Impl,
    #[token("let")]
    Let,
    #[token("const")]
    Const,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("match")]
    Match,
    #[token("loop")]
    Loop,
    #[token("while")]
    While,
    #[token("for")]
    For,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("return")]
    Return,
    #[token("mod")]
    Mod,
    #[token("use")]
    Use,
    #[token("type")]
    Type,
    #[token("pub")]
    Pub,
    #[token("unsafe")]
    Unsafe,
    #[token("as")]
    As,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("self")]
    SelfValue,
    #[token("Self")]
    SelfType,

    // --- keywords (SAT DSL) ---
    #[token("literal")]
    Literal,
    #[token("variable")]
    Variable,
    #[token("formula")]
    Formula,
    #[token("search")]
    Search,
    #[token("scratch")]
    Scratch,
    #[token("bitstruct")]
    Bitstruct,
    #[token("repr_enum")]
    ReprEnum,
    #[token("savepoint")]
    Savepoint,
    #[token("rollback")]
    Rollback,

    // --- literals & idents ---
    #[regex(r"[0-9]+", |lex| lex.slice())]
    Int(&'a str),

    #[regex(r"[_a-zA-Z][_a-zA-Z0-9]*", |lex| lex.slice())]
    Ident(&'a str),

    // --- multi-char operators (before single-char) ---
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("::")]
    PathSep,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,
    #[token("...")]
    DotDotDot,
    #[token("..")]
    DotDot,

    // --- single-char punctuation / operators ---
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("!")]
    Bang,
    #[token("&")]
    Amp,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("@")]
    At,
    #[token("#")]
    Hash,
    #[token("?")]
    Question,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Fn => write!(f, "fn"),
            Token::Struct => write!(f, "struct"),
            Token::Enum => write!(f, "enum"),
            Token::Impl => write!(f, "impl"),
            Token::Let => write!(f, "let"),
            Token::Const => write!(f, "const"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::Match => write!(f, "match"),
            Token::Loop => write!(f, "loop"),
            Token::While => write!(f, "while"),
            Token::For => write!(f, "for"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::Return => write!(f, "return"),
            Token::Mod => write!(f, "mod"),
            Token::Use => write!(f, "use"),
            Token::Type => write!(f, "type"),
            Token::Pub => write!(f, "pub"),
            Token::Unsafe => write!(f, "unsafe"),
            Token::As => write!(f, "as"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::SelfValue => write!(f, "self"),
            Token::SelfType => write!(f, "Self"),
            Token::Literal => write!(f, "literal"),
            Token::Variable => write!(f, "variable"),
            Token::Formula => write!(f, "formula"),
            Token::Search => write!(f, "search"),
            Token::Scratch => write!(f, "scratch"),
            Token::Bitstruct => write!(f, "bitstruct"),
            Token::ReprEnum => write!(f, "repr_enum"),
            Token::Savepoint => write!(f, "savepoint"),
            Token::Rollback => write!(f, "rollback"),
            Token::Int(s) => write!(f, "Int({s})"),
            Token::Ident(s) => write!(f, "Ident({s})"),
            Token::Arrow => write!(f, "->"),
            Token::FatArrow => write!(f, "=>"),
            Token::PathSep => write!(f, "::"),
            Token::EqEq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Le => write!(f, "<="),
            Token::Ge => write!(f, ">="),
            Token::AndAnd => write!(f, "&&"),
            Token::OrOr => write!(f, "||"),
            Token::Shl => write!(f, "<<"),
            Token::Shr => write!(f, ">>"),
            Token::DotDot => write!(f, ".."),
            Token::DotDotDot => write!(f, "..."),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Semi => write!(f, ";"),
            Token::Colon => write!(f, ":"),
            Token::Dot => write!(f, "."),
            Token::Eq => write!(f, "="),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::Bang => write!(f, "!"),
            Token::Amp => write!(f, "&"),
            Token::Pipe => write!(f, "|"),
            Token::Caret => write!(f, "^"),
            Token::At => write!(f, "@"),
            Token::Hash => write!(f, "#"),
            Token::Question => write!(f, "?"),
        }
    }
}

/// Lex `source` into spanned tokens, or return the byte offset of the first error.
pub fn lex(source: &str) -> Result<Vec<SpannedToken<'_>>, usize> {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        match result {
            Ok(kind) => tokens.push(SpannedToken {
                kind,
                span: lexer.span(),
            }),
            Err(()) => return Err(lexer.span().start),
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Token<'_>> {
        lex(source)
            .unwrap_or_else(|off| panic!("lex error at byte {off}"))
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn empty_source() {
        assert!(kinds("").is_empty());
    }

    #[test]
    fn skips_whitespace_and_comments() {
        assert_eq!(
            kinds("  // line\n /* block */ 1"),
            vec![Token::Int("1")]
        );
    }

    #[test]
    fn lexes_fn_main_return() {
        let src = "fn main() -> i32 { return 1 + 1; }";
        assert_eq!(
            kinds(src),
            vec![
                Token::Fn,
                Token::Ident("main"),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("i32"),
                Token::LBrace,
                Token::Return,
                Token::Int("1"),
                Token::Plus,
                Token::Int("1"),
                Token::Semi,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lexes_sat_keywords_and_ops() {
        let src = "literal Lit : u32 { fn not(self) -> Lit { self >> 1 } }";
        assert_eq!(
            kinds(src),
            vec![
                Token::Literal,
                Token::Ident("Lit"),
                Token::Colon,
                Token::Ident("u32"),
                Token::LBrace,
                Token::Fn,
                Token::Ident("not"),
                Token::LParen,
                Token::SelfValue,
                Token::RParen,
                Token::Arrow,
                Token::Ident("Lit"),
                Token::LBrace,
                Token::SelfValue,
                Token::Shr,
                Token::Int("1"),
                Token::RBrace,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lexes_attribute_punct() {
        assert_eq!(
            kinds("#[hot]"),
            vec![Token::Hash, Token::LBracket, Token::Ident("hot"), Token::RBracket]
        );
    }

    #[test]
    fn rejects_unknown_byte() {
        assert_eq!(lex("fn $"), Err(3));
    }
}
