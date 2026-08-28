//! Type checking.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt, Type, UnOp};
use std::collections::HashMap;
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
    Bool,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::I32 => "i32",
            Ty::Bool => "bool",
        }
    }
}

struct Env {
    scopes: Vec<HashMap<String, Ty>>,
}

impl Env {
    fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, ty: Ty) -> Result<(), TypeError> {
        let scope = self.scopes.last_mut().expect("typeck scope");
        if scope.contains_key(name) {
            return Err(TypeError {
                message: format!("duplicate variable `{name}`"),
            });
        }
        scope.insert(name.to_string(), ty);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<Ty> {
        // rev to start from innermost scope and allow shadowing
        self.scopes.iter().rev().find_map(|scope| scope.get(name)).copied()
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
    let mut env = Env::new();
    if check_block(&func.body, ret_ty, &mut env, 0)? != Flow::Return {
        return Err(TypeError {
            message: "missing `return`".into(),
        });
    }
    Ok(())
}

fn resolve_type(ty: &Type) -> Result<Ty, TypeError> {
    match ty {
        Type::Name(name) if name == "i32" => Ok(Ty::I32),
        Type::Name(name) if name == "bool" => Ok(Ty::Bool),
        Type::Name(name) => Err(TypeError {
            message: format!("unknown type `{name}`"),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Next,
    Jump,
    Return,
}

fn join_if(then: Flow, else_: Option<Flow>) -> Flow {
    match else_ {
        None => Flow::Next,
        Some(else_) => match (then, else_) {
            (Flow::Return, Flow::Return) => Flow::Return,
            (Flow::Next, _) | (_, Flow::Next) => Flow::Next,
            _ => Flow::Jump,
        },
    }
}

/// Returns how control leaves `block`.
fn check_block(
    block: &Block,
    return_ty: Ty,
    env: &mut Env,
    loop_depth: u32,
) -> Result<Flow, TypeError> {
    env.push();
    let mut flow = Flow::Next;
    for stmt in &block.stmts {
        flow = check_stmt(stmt, return_ty, env, loop_depth)?;
        if flow != Flow::Next {
            break;
        }
    }
    env.pop();
    Ok(flow)
}

fn check_stmt(
    stmt: &Stmt,
    return_ty: Ty,
    env: &mut Env,
    loop_depth: u32,
) -> Result<Flow, TypeError> {
    match stmt {
        Stmt::Return(expr) => {
            let ty = check_expr(expr, env)?;
            if ty != return_ty {
                return Err(TypeError {
                    message: format!(
                        "return type mismatch: expected `{}`, found `{}`",
                        return_ty.name(),
                        ty.name()
                    ),
                });
            }
            Ok(Flow::Return)
        }
        Stmt::Print(expr) => {
            let ty = check_expr(expr, env)?;
            if ty != Ty::I32 {
                return Err(TypeError {
                    message: format!("`print` requires `i32`, found `{}`", ty.name()),
                });
            }
            Ok(Flow::Next)
        }
        Stmt::Let { name, ty, value } => {
            let value_ty = check_expr(value, env)?;
            let ty = match ty {
                Some(ann) => {
                    let ann_ty = resolve_type(ann)?;
                    if ann_ty != value_ty {
                        return Err(TypeError {
                            message: format!(
                                "variable `{name}` has type `{}` but initializer has type `{}`",
                                ann_ty.name(),
                                value_ty.name()
                            ),
                        });
                    }
                    ann_ty
                }
                None => value_ty,
            };
            env.declare(name, ty)?;
            Ok(Flow::Next)
        }
        Stmt::Assign { name, value } => {
            let Some(var_ty) = env.get(name) else {
                return Err(TypeError {
                    message: format!("undeclared variable `{name}`"),
                });
            };
            let value_ty = check_expr(value, env)?;
            if var_ty != value_ty {
                return Err(TypeError {
                    message: format!(
                        "cannot assign `{}` to `{name}` of type `{}`",
                        value_ty.name(),
                        var_ty.name()
                    ),
                });
            }
            Ok(Flow::Next)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_ty = check_expr(cond, env)?;
            if cond_ty != Ty::Bool {
                return Err(TypeError {
                    message: format!("`if` condition must be `bool`, found `{}`", cond_ty.name()),
                });
            }
            let then_flow = check_block(then_block, return_ty, env, loop_depth)?;
            let else_flow = match else_block {
                Some(else_block) => Some(check_block(else_block, return_ty, env, loop_depth)?),
                None => None,
            };
            Ok(join_if(then_flow, else_flow))
        }
        Stmt::While { cond, body } => {
            let cond_ty = check_expr(cond, env)?;
            if cond_ty != Ty::Bool {
                return Err(TypeError {
                    message: format!(
                        "`while` condition must be `bool`, found `{}`",
                        cond_ty.name()
                    ),
                });
            }
            let _ = check_block(body, return_ty, env, loop_depth + 1)?;
            Ok(Flow::Next)
        }
        Stmt::For {
            name,
            start,
            end,
            body,
        } => {
            let start_ty = check_expr(start, env)?;
            let end_ty = check_expr(end, env)?;
            if start_ty != Ty::I32 || end_ty != Ty::I32 {
                return Err(TypeError {
                    message: format!(
                        "`for` range bounds must be `i32`, found `{}` and `{}`",
                        start_ty.name(),
                        end_ty.name()
                    ),
                });
            }
            env.push();
            env.declare(name, Ty::I32)?;
            let _ = check_block(body, return_ty, env, loop_depth + 1)?;
            env.pop();
            Ok(Flow::Next)
        }
        Stmt::Break => {
            if loop_depth == 0 {
                return Err(TypeError {
                    message: "`break` outside of a loop".into(),
                });
            }
            Ok(Flow::Jump)
        }
        Stmt::Continue => {
            if loop_depth == 0 {
                return Err(TypeError {
                    message: "`continue` outside of a loop".into(),
                });
            }
            Ok(Flow::Jump)
        }
    }
}

fn check_expr(expr: &Expr, env: &Env) -> Result<Ty, TypeError> {
    match expr {
        Expr::Int(value) => {
            if i32::try_from(*value).is_err() {
                return Err(TypeError {
                    message: format!("integer literal `{value}` does not fit in `i32`"),
                });
            }
            Ok(Ty::I32)
        }
        Expr::Bool(_) => Ok(Ty::Bool),
        Expr::Var(name) => env.get(name).ok_or_else(|| TypeError {
            message: format!("undeclared variable `{name}`"),
        }),
        Expr::Unary { op, expr } => {
            let ty = check_expr(expr, env)?;
            match op {
                UnOp::Neg => {
                    if ty != Ty::I32 {
                        return Err(TypeError {
                            message: format!("`-` requires `i32`, found `{}`", ty.name()),
                        });
                    }
                    Ok(Ty::I32)
                }
                UnOp::Not => {
                    if ty != Ty::Bool {
                        return Err(TypeError {
                            message: format!("`!` requires `bool`, found `{}`", ty.name()),
                        });
                    }
                    Ok(Ty::Bool)
                }
            }
        }
        Expr::Binary { op, lhs, rhs } => {
            let lhs_ty = check_expr(lhs, env)?;
            let rhs_ty = check_expr(rhs, env)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                    if lhs_ty != Ty::I32 || rhs_ty != Ty::I32 {
                        return Err(TypeError {
                            message: format!(
                                "`{op}` requires `i32` operands, found `{}` and `{}`",
                                lhs_ty.name(),
                                rhs_ty.name()
                            ),
                        });
                    }
                    Ok(Ty::I32)
                }
                BinOp::Eq | BinOp::Ne => {
                    if lhs_ty != rhs_ty {
                        return Err(TypeError {
                            message: format!(
                                "`{op}` requires matching operands, found `{}` and `{}`",
                                lhs_ty.name(),
                                rhs_ty.name()
                            ),
                        });
                    }
                    Ok(Ty::Bool)
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    if lhs_ty != Ty::I32 || rhs_ty != Ty::I32 {
                        return Err(TypeError {
                            message: format!(
                                "`{op}` requires `i32` operands, found `{}` and `{}`",
                                lhs_ty.name(),
                                rhs_ty.name()
                            ),
                        });
                    }
                    Ok(Ty::Bool)
                }
                BinOp::And | BinOp::Or => {
                    if lhs_ty != Ty::Bool || rhs_ty != Ty::Bool {
                        return Err(TypeError {
                            message: format!(
                                "`{op}` requires `bool` operands, found `{}` and `{}`",
                                lhs_ty.name(),
                                rhs_ty.name()
                            ),
                        });
                    }
                    Ok(Ty::Bool)
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
        let err = check("fn main() -> usize { return 1; }").unwrap_err();
        assert!(err.message.contains("unknown type"), "{}", err.message);
    }

    #[test]
    fn rejects_i32_overflow_literal() {
        let err = check("fn main() -> i32 { return 9999999999; }").unwrap_err();
        assert!(err.message.contains("i32"), "{}", err.message);
    }

    #[test]
    fn accepts_print() {
        assert!(check("fn main() -> i32 { print(1 + 1); return 0; }").is_ok());
    }

    #[test]
    fn accepts_let_if_while() {
        assert!(check(
            "fn main() -> i32 { let i = 0; while i < 3 { i = i + 1; } if i == 3 { return 1; } else { return 0; } }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_if_on_i32() {
        let err = check("fn main() -> i32 { if 1 { return 0; } return 1; }").unwrap_err();
        assert!(err.message.contains("bool"), "{}", err.message);
    }

    #[test]
    fn rejects_undeclared_variable() {
        let err = check("fn main() -> i32 { return x; }").unwrap_err();
        assert!(err.message.contains("undeclared"), "{}", err.message);
    }

    #[test]
    fn if_else_both_return() {
        assert!(check("fn main() -> i32 { if true { return 1; } else { return 0; } }").is_ok());
    }

    #[test]
    fn ignores_dead_code_after_return() {
        assert!(check("fn main() -> i32 { return 0; print(true); if 1 { return x; } }").is_ok());
    }

    #[test]
    fn accepts_integer_arithmetic() {
        assert!(check("fn main() -> i32 { return -1 + 2 * 3 - 8 / 2 % 3; }").is_ok());
    }

    #[test]
    fn rejects_bool_arithmetic() {
        let err = check("fn main() -> i32 { return true * 1; }").unwrap_err();
        assert!(err.message.contains("i32"), "{}", err.message);
    }

    #[test]
    fn rejects_unary_minus_on_bool() {
        let err = check("fn main() -> i32 { return -true; }").unwrap_err();
        assert!(err.message.contains("i32"), "{}", err.message);
    }

    #[test]
    fn accepts_logic() {
        assert!(check(
            "fn main() -> i32 { if true && false || !false { return 1; } return 0; }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_and_on_i32() {
        let err = check("fn main() -> i32 { if 1 && true { return 0; } return 1; }").unwrap_err();
        assert!(err.message.contains("bool"), "{}", err.message);
    }

    #[test]
    fn rejects_not_on_i32() {
        let err = check("fn main() -> i32 { if !1 { return 0; } return 1; }").unwrap_err();
        assert!(err.message.contains("bool"), "{}", err.message);
    }

    #[test]
    fn accepts_for_break_continue() {
        assert!(check(
            "fn main() -> i32 { for i in 0..3 { if i == 1 { continue; } if i == 2 { break; } } return 0; }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_break_outside_loop() {
        let err = check("fn main() -> i32 { break; return 0; }").unwrap_err();
        assert!(err.message.contains("break"), "{}", err.message);
    }

    #[test]
    fn rejects_for_bool_range() {
        let err = check("fn main() -> i32 { for i in true..1 { } return 0; }").unwrap_err();
        assert!(err.message.contains("i32"), "{}", err.message);
    }
}
