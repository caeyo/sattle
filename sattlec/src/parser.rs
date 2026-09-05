//! Recursive-descent parser.

use crate::ast::{
    BinOp, Block, Expr, Field, Function, Item, Module, Param, Stmt, StructItem, Type, UnOp,
};
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

    fn peek_nth_kind(&self, n: usize) -> Option<&Token<'a>> {
        self.tokens.get(self.pos + n).map(|t| &t.kind)
    }

    fn at_struct_lit(&self) -> bool {
        matches!(self.peek_kind(), Some(Token::LBrace))
            && matches!(self.peek_nth_kind(1), Some(Token::Ident(_)))
            && matches!(self.peek_nth_kind(2), Some(Token::Colon))
    }

    fn current_offset(&self) -> usize {
        self.peek().map(|t| t.span.start).unwrap_or(self.eof_offset)
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

    fn expect(
        &mut self,
        expected: &Token<'_>,
        label: &str,
    ) -> Result<&'a SpannedToken<'a>, ParseError> {
        match self.peek_kind() {
            Some(kind) if std::mem::discriminant(kind) == std::mem::discriminant(expected) => {
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
            Some(Token::Struct) => Ok(Item::Struct(self.parse_struct()?)),
            Some(kind) => Err(self.error(format!("expected item, found {kind}"))),
            None => Err(self.error("expected item, found end of file")),
        }
    }

    fn parse_struct(&mut self) -> Result<StructItem, ParseError> {
        self.expect(&Token::Struct, "`struct`")?;
        let name = self.expect_ident("struct name")?;
        self.expect(&Token::LBrace, "`{`")?;
        if matches!(self.peek_kind(), Some(Token::RBrace)) {
            return Err(self.error(format!("struct `{name}` must have at least one field")));
        }
        let mut fields = Vec::new();
        loop {
            let field_name = self.expect_ident("field name")?;
            self.expect(&Token::Colon, "`:`")?;
            let ty = self.parse_type()?;
            fields.push(Field {
                name: field_name,
                ty,
            });
            if matches!(self.peek_kind(), Some(Token::Comma)) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect(&Token::RBrace, "`}`")?;
        Ok(StructItem { name, fields })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(&Token::Fn, "`fn`")?;
        let name = self.expect_ident("function name")?;
        let params = self.parse_params()?;
        self.expect(&Token::Arrow, "`->`")?;
        let return_ty = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(Function {
            name,
            params,
            return_ty,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(&Token::LParen, "`(`")?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), Some(Token::RParen)) {
            loop {
                let name = self.expect_ident("parameter name")?;
                self.expect(&Token::Colon, "`:`")?;
                let ty = self.parse_type()?;
                params.push(Param { name, ty });
                if matches!(self.peek_kind(), Some(Token::Comma)) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect(&Token::RParen, "`)`")?;
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        if matches!(self.peek_kind(), Some(Token::Star)) {
            self.bump();
            return Ok(Type::Ptr(Box::new(self.parse_type()?)));
        }
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
            Some(Token::Print) => {
                self.bump();
                self.expect(&Token::LParen, "`(`")?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen, "`)`")?;
                self.expect(&Token::Semi, "`;`")?;
                Ok(Stmt::Print(expr))
            }
            Some(Token::Let) => self.parse_let(),
            Some(Token::If) => self.parse_if(),
            Some(Token::While) => self.parse_while(),
            Some(Token::For) => self.parse_for(),
            Some(Token::Break) => {
                self.bump();
                self.expect(&Token::Semi, "`;`")?;
                Ok(Stmt::Break)
            }
            Some(Token::Continue) => {
                self.bump();
                self.expect(&Token::Semi, "`;`")?;
                Ok(Stmt::Continue)
            }
            Some(_) => self.parse_assign(),
            None => Err(self.error("expected statement, found end of file")),
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&Token::Let, "`let`")?;
        let name = self.expect_ident("variable name")?;
        let ty = if matches!(self.peek_kind(), Some(Token::Colon)) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&Token::Eq, "`=`")?;
        let value = self.parse_expr()?;
        self.expect(&Token::Semi, "`;`")?;
        Ok(Stmt::Let { name, ty, value })
    }

    fn parse_assign(&mut self) -> Result<Stmt, ParseError> {
        let target = self.parse_expr()?;
        self.expect(&Token::Eq, "`=`")?;
        let value = self.parse_expr()?;
        self.expect(&Token::Semi, "`;`")?;
        Ok(Stmt::Assign { target, value })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&Token::If, "`if`")?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if matches!(self.peek_kind(), Some(Token::Else)) {
            self.bump();
            if matches!(self.peek_kind(), Some(Token::If)) {
                Some(Block {
                    stmts: vec![self.parse_if()?],
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_block,
            else_block,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&Token::While, "`while`")?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&Token::For, "`for`")?;
        let name = self.expect_ident("loop variable")?;
        self.expect(&Token::In, "`in`")?;
        let start = self.parse_expr()?;
        self.expect(&Token::DotDot, "`..`")?;
        let end = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            name,
            start,
            end,
            body,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek_kind(), Some(Token::OrOr)) {
            self.bump();
            let rhs = self.parse_and()?;
            lhs = Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_cmp()?;
        while matches!(self.peek_kind(), Some(Token::AndAnd)) {
            self.bump();
            let rhs = self.parse_cmp()?;
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_add()?;
        while let Some(op) = self.peek_cmp_op() {
            self.bump();
            let rhs = self.parse_add()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn peek_cmp_op(&self) -> Option<BinOp> {
        match self.peek_kind() {
            Some(Token::EqEq) => Some(BinOp::Eq),
            Some(Token::NotEq) => Some(BinOp::Ne),
            Some(Token::Lt) => Some(BinOp::Lt),
            Some(Token::Le) => Some(BinOp::Le),
            Some(Token::Gt) => Some(BinOp::Gt),
            Some(Token::Ge) => Some(BinOp::Ge),
            _ => None,
        }
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_mul()?;
        while let Some(op) = self.peek_add_op() {
            self.bump();
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn peek_add_op(&self) -> Option<BinOp> {
        match self.peek_kind() {
            Some(Token::Plus) => Some(BinOp::Add),
            Some(Token::Minus) => Some(BinOp::Sub),
            _ => None,
        }
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;
        while let Some(op) = self.peek_mul_op() {
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn peek_mul_op(&self) -> Option<BinOp> {
        match self.peek_kind() {
            Some(Token::Star) => Some(BinOp::Mul),
            Some(Token::Slash) => Some(BinOp::Div),
            Some(Token::Percent) => Some(BinOp::Rem),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind() {
            Some(Token::Minus) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Some(Token::Bang) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Not,
                    expr: Box::new(expr),
                })
            }
            Some(Token::Star) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Deref,
                    expr: Box::new(expr),
                })
            }
            Some(Token::Amp) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::AddrOf,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_operand()?;
        while matches!(self.peek_kind(), Some(Token::Dot)) {
            self.bump();
            let field = self.expect_ident("field name")?;
            expr = Expr::Field {
                base: Box::new(expr),
                field,
            };
        }
        Ok(expr)
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
            Some(Token::True) => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Some(Token::False) => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Some(Token::Ident(name)) => {
                let name = (*name).to_string();
                self.bump();
                if matches!(self.peek_kind(), Some(Token::LParen)) {
                    let args = self.parse_arg_list()?;
                    Ok(Expr::Call { name, args })
                } else if self.at_struct_lit() {
                    let fields = self.parse_struct_lit_fields()?;
                    Ok(Expr::StructLit { name, fields })
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Some(Token::LParen) => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen, "`)`")?;
                Ok(expr)
            }
            Some(kind) => Err(self.error(format!("expected expression, found {kind}"))),
            None => Err(self.error("expected expression, found end of file")),
        }
    }

    fn parse_struct_lit_fields(&mut self) -> Result<Vec<(String, Expr)>, ParseError> {
        self.expect(&Token::LBrace, "`{`")?;
        let mut fields = Vec::new();
        loop {
            let name = self.expect_ident("field name")?;
            self.expect(&Token::Colon, "`:`")?;
            let value = self.parse_expr()?;
            fields.push((name, value));
            if matches!(self.peek_kind(), Some(Token::Comma)) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect(&Token::RBrace, "`}`")?;
        Ok(fields)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        self.expect(&Token::LParen, "`(`")?;
        let mut args = Vec::new();
        if !matches!(self.peek_kind(), Some(Token::RParen)) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek_kind(), Some(Token::Comma)) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect(&Token::RParen, "`)`")?;
        Ok(args)
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
    use crate::ast::{format_ast, BinOp, Expr, Function, Item, Stmt, Type, UnOp};
    use crate::lexer::lex;

    fn parse_src(src: &str) -> Module {
        let tokens = lex(src).unwrap_or_else(|off| panic!("lex error at {off}"));
        parse(&tokens, src.len()).unwrap_or_else(|e| panic!("{} at {}", e.message, e.offset))
    }

    fn first_fn(module: &Module) -> &Function {
        module
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(func) => Some(func),
                Item::Struct(_) => None,
            })
            .expect("function")
    }

    #[test]
    fn parses_add_example() {
        let module = parse_src("fn main() -> i32 {\n    return 1 + 1;\n}\n");
        assert_eq!(module.items.len(), 1);
        let func = first_fn(&module);
        assert_eq!(func.name, "main");
        assert_eq!(func.return_ty, Type::Name("i32".into()));
        assert_eq!(func.body.stmts.len(), 1);
        let Stmt::Return(expr) = &func.body.stmts[0] else {
            panic!("expected return");
        };
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
        let func = first_fn(&module);
        let Stmt::Return(expr) = &func.body.stmts[0] else {
            panic!("expected return");
        };
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

    #[test]
    fn parses_print() {
        let module = parse_src("fn main() -> i32 { print(1 + 1); return 0; }");
        let func = first_fn(&module);
        assert!(matches!(&func.body.stmts[0], Stmt::Print(_)));
        assert!(matches!(&func.body.stmts[1], Stmt::Return(_)));
    }

    #[test]
    fn parses_let_if_while() {
        let module = parse_src(
            "fn main() -> i32 {\n    let i = 0;\n    while i < 3 { i = i + 1; }\n    if i == 3 { return 1; } else { return 0; }\n}",
        );
        let func = first_fn(&module);
        assert!(matches!(&func.body.stmts[0], Stmt::Let { name, .. } if name == "i"));
        assert!(matches!(&func.body.stmts[1], Stmt::While { .. }));
        assert!(matches!(&func.body.stmts[2], Stmt::If { .. }));
    }

    #[test]
    fn parses_for_break_continue() {
        let module = parse_src(
            "fn main() -> i32 { for i in 0..3 { if i == 1 { continue; } break; } return 0; }",
        );
        let func = first_fn(&module);
        assert!(matches!(&func.body.stmts[0], Stmt::For { name, .. } if name == "i"));
        let Stmt::For { body, .. } = &func.body.stmts[0] else {
            panic!("expected for");
        };
        assert!(matches!(&body.stmts[1], Stmt::Break));
    }

    #[test]
    fn parses_params_and_call() {
        let module = parse_src(
            "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(1, 2); }",
        );
        assert_eq!(module.items.len(), 2);
        let add = first_fn(&module);
        assert_eq!(add.params.len(), 2);
        assert_eq!(add.params[0].name, "a");
        assert_eq!(
            return_expr("fn main() -> i32 { return add(1, 2); }"),
            Expr::Call {
                name: "add".into(),
                args: vec![Expr::Int(1), Expr::Int(2)],
            }
        );
    }

    fn return_expr(src: &str) -> Expr {
        let module = parse_src(src);
        let func = first_fn(&module);
        let Stmt::Return(expr) = &func.body.stmts[0] else {
            panic!("expected return");
        };
        expr.clone()
    }

    #[test]
    fn mul_binds_tighter_than_add() {
        assert_eq!(
            return_expr("fn main() -> i32 { return 1 + 2 * 3; }"),
            Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Binary {
                    op: BinOp::Mul,
                    lhs: Box::new(Expr::Int(2)),
                    rhs: Box::new(Expr::Int(3)),
                }),
            }
        );
    }

    #[test]
    fn unary_minus_binds_tighter_than_mul() {
        assert_eq!(
            return_expr("fn main() -> i32 { return -2 * 3; }"),
            Expr::Binary {
                op: BinOp::Mul,
                lhs: Box::new(Expr::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(Expr::Int(2)),
                }),
                rhs: Box::new(Expr::Int(3)),
            }
        );
    }

    #[test]
    fn mul_div_are_left_associative() {
        assert_eq!(
            return_expr("fn main() -> i32 { return 8 / 2 * 2; }"),
            Expr::Binary {
                op: BinOp::Mul,
                lhs: Box::new(Expr::Binary {
                    op: BinOp::Div,
                    lhs: Box::new(Expr::Int(8)),
                    rhs: Box::new(Expr::Int(2)),
                }),
                rhs: Box::new(Expr::Int(2)),
            }
        );
    }

    #[test]
    fn sub_is_left_associative() {
        assert_eq!(
            return_expr("fn main() -> i32 { return 10 - 3 - 2; }"),
            Expr::Binary {
                op: BinOp::Sub,
                lhs: Box::new(Expr::Binary {
                    op: BinOp::Sub,
                    lhs: Box::new(Expr::Int(10)),
                    rhs: Box::new(Expr::Int(3)),
                }),
                rhs: Box::new(Expr::Int(2)),
            }
        );
    }

    fn if_cond(src: &str) -> Expr {
        let module = parse_src(src);
        let func = first_fn(&module);
        let Stmt::If { cond, .. } = &func.body.stmts[0] else {
            panic!("expected if");
        };
        cond.clone()
    }

    #[test]
    fn and_binds_tighter_than_or() {
        assert_eq!(
            if_cond("fn main() -> i32 { if true || false && false { return 1; } return 0; }"),
            Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(Expr::Bool(true)),
                rhs: Box::new(Expr::Binary {
                    op: BinOp::And,
                    lhs: Box::new(Expr::Bool(false)),
                    rhs: Box::new(Expr::Bool(false)),
                }),
            }
        );
    }

    #[test]
    fn not_binds_tighter_than_and() {
        assert_eq!(
            if_cond("fn main() -> i32 { if !true && false { return 1; } return 0; }"),
            Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(Expr::Unary {
                    op: UnOp::Not,
                    expr: Box::new(Expr::Bool(true)),
                }),
                rhs: Box::new(Expr::Bool(false)),
            }
        );
    }

    #[test]
    fn parses_struct_ptr_and_fields() {
        let module = parse_src(
            "struct Point { x: i32, y: i32 } fn bump(p: *Point) -> i32 { p.x = p.x + 1; return p.x; } fn main() -> i32 { let p = Point { x: 1, y: 2 }; return bump(&p); }",
        );
        assert!(matches!(&module.items[0], Item::Struct(def) if def.name == "Point"));
        let bump = first_fn(&module);
        assert_eq!(
            bump.params[0].ty,
            Type::Ptr(Box::new(Type::Name("Point".into())))
        );
        assert!(matches!(
            &bump.body.stmts[0],
            Stmt::Assign {
                target: Expr::Field { field, .. },
                ..
            } if field == "x"
        ));
        assert_eq!(
            return_expr("fn main() -> i32 { return Point { x: 1, y: 2 }.x; }"),
            Expr::Field {
                base: Box::new(Expr::StructLit {
                    name: "Point".into(),
                    fields: vec![("x".into(), Expr::Int(1)), ("y".into(), Expr::Int(2)),],
                }),
                field: "x".into(),
            }
        );
        assert_eq!(
            return_expr("fn main() -> i32 { return *p; }"),
            Expr::Unary {
                op: UnOp::Deref,
                expr: Box::new(Expr::Var("p".into())),
            }
        );
        assert_eq!(
            return_expr("fn main() -> i32 { return &p; }"),
            Expr::Unary {
                op: UnOp::AddrOf,
                expr: Box::new(Expr::Var("p".into())),
            }
        );
    }

    #[test]
    fn if_does_not_parse_struct_lit() {
        let module = parse_src("fn main() -> i32 { if p { return 1; } return 0; }");
        let func = first_fn(&module);
        let Stmt::If { cond, .. } = &func.body.stmts[0] else {
            panic!("expected if");
        };
        assert_eq!(cond, &Expr::Var("p".into()));
    }

    #[test]
    fn if_parses_struct_lit_when_fields_follow() {
        let module =
            parse_src("fn main() -> i32 { if Point { x: 1 }.x == 1 { return 1; } return 0; }");
        let func = first_fn(&module);
        let Stmt::If { cond, .. } = &func.body.stmts[0] else {
            panic!("expected if");
        };
        assert!(matches!(
            cond,
            Expr::Binary {
                op: BinOp::Eq,
                lhs,
                ..
            } if matches!(&**lhs, Expr::Field { field, base } if field == "x" && matches!(&**base, Expr::StructLit { name, .. } if name == "Point"))
        ));
    }

    #[test]
    fn rejects_empty_struct() {
        let src = "struct Point {} fn main() -> i32 { return 0; }";
        let tokens = lex(src).unwrap();
        let err = parse(&tokens, src.len()).unwrap_err();
        assert!(
            err.message.contains("at least one field"),
            "{}",
            err.message
        );
    }
}
