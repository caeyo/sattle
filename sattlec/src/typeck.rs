//! Type checking.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt, Type};
use std::collections::HashSet;

/// A type-checking error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    pub message: String,
}

/// Resolved types (after name resolution of type syntax).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    I32,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::I32 => "i32",
        }
    }
}

/// Type-check a module.
pub fn typeck(module: &Module) -> Result<(), TypeError> {
    let mut names = HashSet::new();

    for item in &module.items {
        match item {
            Item::Fn(func) => {
                if !names.insert(func.name.as_str()) {
                    return Err(TypeError {
                        message: format!("duplicate definition of `{}`", func.name),
                    });
                }
                check_function(func)?;
            }
        }
    }

    Ok(())
}

fn check_function(func: &Function) -> Result<(), TypeError> {
    let ret_ty = resolve_type(&func.return_ty)?;
    check_block(&func.body, ret_ty)
}

fn resolve_type(ty: &Type) -> Result<Ty, TypeError> {
    match ty {
        Type::Name(name) if name == "i32" => Ok(Ty::I32),
        Type::Name(name) => Err(TypeError {
            message: format!("unknown type `{name}`"),
        }),
    }
}

fn check_block(block: &Block, return_ty: Ty) -> Result<(), TypeError> {
    if block.stmts.is_empty() {
        return Err(TypeError {
            message: "missing `return`".into(),
        });
    }
    for stmt in &block.stmts {
        check_stmt(stmt, return_ty)?;
    }
    Ok(())
}

fn check_stmt(stmt: &Stmt, return_ty: Ty) -> Result<(), TypeError> {
    match stmt {
        Stmt::Return(expr) => {
            let ty = check_expr(expr)?;
            if ty != return_ty {
                return Err(TypeError {
                    message: format!(
                        "return type mismatch: expected `{}`, found `{}`",
                        return_ty.name(),
                        ty.name()
                    ),
                });
            }
            Ok(())
        }
    }
}

fn check_expr(expr: &Expr) -> Result<Ty, TypeError> {
    match expr {
        Expr::Int(value) => {
            if i32::try_from(*value).is_err() {
                return Err(TypeError {
                    message: format!("integer literal `{value}` does not fit in `i32`"),
                });
            }
            Ok(Ty::I32)
        }
        Expr::Binary { op, lhs, rhs } => {
            let lhs_ty = check_expr(lhs)?;
            let rhs_ty = check_expr(rhs)?;
            match op {
                BinOp::Add => {
                    if lhs_ty != Ty::I32 || rhs_ty != Ty::I32 {
                        return Err(TypeError {
                            message: format!(
                                "`+` requires `i32` operands, found `{}` and `{}`",
                                lhs_ty.name(),
                                rhs_ty.name()
                            ),
                        });
                    }
                    Ok(Ty::I32)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn check(src: &str) -> Result<(), TypeError> {
        let tokens = lex(src).unwrap();
        let module = parse(&tokens, src.len()).unwrap();
        typeck(&module)
    }

    #[test]
    fn accepts_main_returning_i32() {
        assert!(check("fn main() -> i32 { return 1 + 1; }").is_ok());
    }

    #[test]
    fn accepts_non_main_function() {
        assert!(check("fn add() -> i32 { return 1 + 1; }").is_ok());
    }

    #[test]
    fn accepts_empty_module() {
        assert!(check("").is_ok());
    }

    #[test]
    fn rejects_duplicate_function() {
        let err = check("fn add() -> i32 { return 1; } fn add() -> i32 { return 2; }").unwrap_err();
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_type() {
        let err = check("fn main() -> bool { return 1; }").unwrap_err();
        assert!(err.message.contains("unknown type"), "{}", err.message);
    }

    #[test]
    fn rejects_i32_overflow_literal() {
        let err = check("fn main() -> i32 { return 9999999999; }").unwrap_err();
        assert!(err.message.contains("i32"), "{}", err.message);
    }
}
