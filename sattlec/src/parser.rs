//! Recursive-descent parser.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt, Type};
use crate::lexer::{SpannedToken, Token};

/// Parse error: message and byte offset into the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

struct Parser<'a> {
    tokens: &'a [SpannedToken<'a>],
    pos: usize,
    /// Byte length of the source (EOF location).
    eof_offset: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [SpannedToken<'a>], source_len: usize) -> Self {
        Self {
            tokens,
            pos: 0,
            eof_offset: source_len,
        }
    }

    fn peek(&self) -> Option<&SpannedToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&Token<'a>> {
        self.peek().map(|t| &t.kind)
    }

    fn current_offset(&self) -> usize {
        self.peek()
            .map(|t| t.span.start)
            .unwrap_or(self.eof_offset)
    }

    fn bump(&mut self) -> Option<&'a SpannedToken<'a>> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: message.into(),
            offset: self.current_offset(),
        }
    }

    fn expect(&mut self, expected: &Token<'_>, label: &str) -> Result<&'a SpannedToken<'a>, ParseError> {
        match self.peek_kind() {
            Some(kind)
                if std::mem::discriminant(kind) == std::mem::discriminant(expected) =>
            {
                Ok(self.bump().unwrap())
            }
            Some(kind) => Err(self.error(format!("expected {label}, found {kind}"))),
            None => Err(self.error(format!("expected {label}, found end of file"))),
        }
    }

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(Module { items })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek_kind() {
            Some(Token::Fn) => Ok(Item::Fn(self.parse_function()?)),
            Some(kind) => Err(self.error(format!("expected item, found {kind}"))),
            None => Err(self.error("expected item, found end of file")),
        }
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(&Token::Fn, "`fn`")?;
        let name = self.expect_ident("function name")?;
        self.expect(&Token::LParen, "`(`")?;
        self.expect(&Token::RParen, "`)`")?;
        self.expect(&Token::Arrow, "`->`")?;
        let return_ty = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(Function {
            name,
            return_ty,
            body,
        })
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        Ok(Type::Name(self.expect_ident("type name")?))
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect(&Token::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek_kind(), Some(Token::RBrace) | None) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace, "`}`")?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek_kind() {
            Some(Token::Return) => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semi, "`;`")?;
                Ok(Stmt::Return(expr))
            }
            Some(kind) => Err(self.error(format!("expected statement, found {kind}"))),
            None => Err(self.error("expected statement, found end of file")),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_add()
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_operand()?;
        while matches!(self.peek_kind(), Some(Token::Plus)) {
            self.bump();
            let rhs = self.parse_operand()?;
            lhs = Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_operand(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind() {
            Some(Token::Int(digits)) => {
                let digits = *digits;
                let offset = self.current_offset();
                self.bump();
                let value = digits.parse::<i64>().map_err(|_| ParseError {
                    message: format!("integer literal `{digits}` is out of range"),
                    offset,
                })?;
                Ok(Expr::Int(value))
            }
            Some(kind) => Err(self.error(format!("expected expression, found {kind}"))),
            None => Err(self.error("expected expression, found end of file")),
        }
    }

    fn expect_ident(&mut self, label: &str) -> Result<String, ParseError> {
        match self.peek_kind() {
            Some(Token::Ident(name)) => {
                let name = (*name).to_string();
                self.bump();
                Ok(name)
            }
            Some(kind) => Err(self.error(format!("expected {label}, found {kind}"))),
            None => Err(self.error(format!("expected {label}, found end of file"))),
        }
    }
}

/// Parse a token stream into a module AST.
pub fn parse(tokens: &[SpannedToken<'_>], source_len: usize) -> Result<Module, ParseError> {
    let mut parser = Parser::new(tokens, source_len);
    parser.parse_module()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{format_ast, BinOp, Expr, Item, Stmt};
    use crate::lexer::lex;

    fn parse_src(src: &str) -> Module {
        let tokens = lex(src).unwrap_or_else(|off| panic!("lex error at {off}"));
        parse(&tokens, src.len()).unwrap_or_else(|e| panic!("{} at {}", e.message, e.offset))
    }

    #[test]
    fn parses_add_example() {
        let module = parse_src("fn main() -> i32 {\n    return 1 + 1;\n}\n");
        assert_eq!(module.items.len(), 1);
        let Item::Fn(func) = &module.items[0];
        assert_eq!(func.name, "main");
        assert_eq!(func.return_ty, Type::Name("i32".into()));
        assert_eq!(func.body.stmts.len(), 1);
        let Stmt::Return(expr) = &func.body.stmts[0];
        assert_eq!(
            expr,
            &Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(1)),
            }
        );
    }

    #[test]
    fn addition_is_left_associative() {
        let module = parse_src("fn main() -> i32 { return 1 + 2 + 3; }");
        let Item::Fn(func) = &module.items[0];
        let Stmt::Return(expr) = &func.body.stmts[0];
        assert_eq!(
            expr,
            &Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(Expr::Binary {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Int(1)),
                    rhs: Box::new(Expr::Int(2)),
                }),
                rhs: Box::new(Expr::Int(3)),
            }
        );
    }

    #[test]
    fn format_ast_is_stable() {
        let src = "fn main() -> i32 { return 1 + 1; }";
        let module = parse_src(src);
        assert_eq!(
            format_ast(&module),
            "\
Module
  Fn main
    ReturnType
      Name(i32)
    Body
      Block
        Return
          Binary(+)
            Int(1)
            Int(1)
"
        );
    }

    #[test]
    fn rejects_missing_semicolon() {
        let src = "fn main() -> i32 { return 1 }";
        let tokens = lex(src).unwrap();
        let err = parse(&tokens, src.len()).unwrap_err();
        assert!(err.message.contains("`;`"), "{}", err.message);
    }

    #[test]
    fn empty_module() {
        let module = parse_src("");
        assert!(module.items.is_empty());
    }
}
