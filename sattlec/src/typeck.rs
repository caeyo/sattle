//! Type checking.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt, Type, UnOp};
use std::collections::{HashMap, HashSet};

/// A type-checking error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    pub message: String,
}

/// Resolved types (after name resolution of type syntax).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    I32,
    Bool,
    Struct(String),
    Ptr(Box<Ty>),
}

impl Ty {
    pub fn name(&self) -> String {
        match self {
            Ty::I32 => "i32".into(),
            Ty::Bool => "bool".into(),
            Ty::Struct(name) => name.clone(),
            Ty::Ptr(inner) => format!("*{}", inner.name()),
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
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
    }
}

struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
}

struct StructDef {
    fields: Vec<(String, Ty)>,
}

impl StructDef {
    fn field(&self, name: &str) -> Option<&Ty> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, ty)| ty)
    }
}

/// Type-check a module.
pub fn typeck(module: &Module) -> Result<(), TypeError> {
    let structs = collect_structs(module)?;
    let fns = collect_fns(module, &structs)?;

    for item in &module.items {
        match item {
            Item::Fn(func) => {
                let mut checker = Checker {
                    env: Env::new(),
                    fns: &fns,
                    structs: &structs,
                    return_ty: fns[&func.name].ret.clone(),
                    loop_depth: 0,
                };
                checker.check_function(func)?;
            }
            Item::Struct(_) => {}
        }
    }

    Ok(())
}

fn collect_structs(module: &Module) -> Result<HashMap<String, StructDef>, TypeError> {
    let mut structs = HashMap::new();
    for item in &module.items {
        if let Item::Struct(def) = item {
            if def.name == "i32" || def.name == "bool" {
                return Err(TypeError {
                    message: format!("cannot define struct with reserved name `{}`", def.name),
                });
            }
            if structs.contains_key(&def.name) {
                return Err(TypeError {
                    message: format!("duplicate definition of `{}`", def.name),
                });
            }
            structs.insert(def.name.clone(), StructDef { fields: Vec::new() });
        }
    }

    // Grab fields on second pass so that a field can name a struct that is defined later
    for item in &module.items {
        if let Item::Struct(def) = item {
            let mut fields = Vec::new();
            let mut seen = HashSet::new();
            for field in &def.fields {
                if !seen.insert(field.name.clone()) {
                    return Err(TypeError {
                        message: format!("duplicate field `{}` on `{}`", field.name, def.name),
                    });
                }
                fields.push((field.name.clone(), resolve_type(&field.ty, &structs)?));
            }
            if fields.is_empty() {
                return Err(TypeError {
                    message: format!("struct `{}` must have at least one field", def.name),
                });
            }
            structs.get_mut(&def.name).unwrap().fields = fields;
        }
    }

    let names: Vec<String> = structs.keys().cloned().collect();
    let mut done = HashSet::new();
    let mut stack = Vec::new();
    for name in &names {
        check_finite(name, &structs, &mut stack, &mut done)?;
    }

    Ok(structs)
}

fn check_finite(
    name: &str,
    structs: &HashMap<String, StructDef>,
    stack: &mut Vec<String>,
    done: &mut HashSet<String>,
) -> Result<(), TypeError> {
    if done.contains(name) {
        return Ok(());
    }
    if stack.iter().any(|seen| seen == name) {
        return Err(TypeError {
            message: format!("recursive type `{name}` without indirection"),
        });
    }
    stack.push(name.to_string());
    for (_, ty) in &structs[name].fields {
        if let Ty::Struct(inner) = ty {
            check_finite(inner, structs, stack, done)?;
        }
    }
    stack.pop();
    done.insert(name.to_string());
    Ok(())
}

fn collect_fns(
    module: &Module,
    structs: &HashMap<String, StructDef>,
) -> Result<HashMap<String, FnSig>, TypeError> {
    let mut fns = HashMap::new();
    for item in &module.items {
        if let Item::Fn(func) = item {
            if fns.contains_key(&func.name) {
                return Err(TypeError {
                    message: format!("duplicate definition of `{}`", func.name),
                });
            }
            let mut params = Vec::new();
            for param in &func.params {
                params.push(resolve_type(&param.ty, structs)?);
            }
            let ret = resolve_type(&func.return_ty, structs)?;
            fns.insert(func.name.clone(), FnSig { params, ret });
        }
    }
    Ok(fns)
}

fn resolve_type(ty: &Type, structs: &HashMap<String, StructDef>) -> Result<Ty, TypeError> {
    match ty {
        Type::Name(name) if name == "i32" => Ok(Ty::I32),
        Type::Name(name) if name == "bool" => Ok(Ty::Bool),
        Type::Name(name) if structs.contains_key(name) => Ok(Ty::Struct(name.clone())),
        Type::Name(name) => Err(TypeError {
            message: format!("unknown type `{name}`"),
        }),
        Type::Ptr(inner) => Ok(Ty::Ptr(Box::new(resolve_type(inner, structs)?))),
    }
}

struct Checker<'a> {
    env: Env,
    fns: &'a HashMap<String, FnSig>,
    structs: &'a HashMap<String, StructDef>,
    return_ty: Ty,
    loop_depth: u32,
}

impl<'a> Checker<'a> {
    fn check_function(&mut self, func: &Function) -> Result<(), TypeError> {
        self.env.push();
        for param in &func.params {
            self.env
                .declare(&param.name, resolve_type(&param.ty, self.structs)?)?;
        }
        if self.check_block(&func.body)? != Flow::Return {
            return Err(TypeError {
                message: "missing `return`".into(),
            });
        }
        self.env.pop();
        Ok(())
    }

    fn resolve(&self, ty: &Type) -> Result<Ty, TypeError> {
        resolve_type(ty, self.structs)
    }

    fn field_ty(&self, base_ty: &Ty, field: &str) -> Result<Ty, TypeError> {
        let mut ty = base_ty;
        loop {
            match ty {
                Ty::Struct(name) => {
                    let def = &self.structs[name];
                    return def.field(field).cloned().ok_or_else(|| TypeError {
                        message: format!("no field `{field}` on `{name}`"),
                    });
                }
                Ty::Ptr(inner) => ty = inner,
                _ => {
                    return Err(TypeError {
                        message: format!(
                            "field access requires a struct, found `{}`",
                            base_ty.name()
                        ),
                    });
                }
            }
        }
    }

    fn check_place(&mut self, expr: &Expr) -> Result<Ty, TypeError> {
        match expr {
            Expr::Var(name) => self.env.get(name).ok_or_else(|| TypeError {
                message: format!("undeclared variable `{name}`"),
            }),
            Expr::Unary {
                op: UnOp::Deref,
                expr,
            } => match self.check_expr(expr)? {
                Ty::Ptr(inner) => Ok(*inner),
                ty => Err(TypeError {
                    message: format!("`*` requires a pointer, found `{}`", ty.name()),
                }),
            },
            Expr::Field { base, field } => {
                let base_ty = self.check_expr(base)?;
                let field_ty = self.field_ty(&base_ty, field)?;
                if self.check_place(base).is_ok() || matches!(base_ty, Ty::Ptr(_)) {
                    Ok(field_ty)
                } else {
                    Err(TypeError {
                        message: "cannot assign to this expression".into(),
                    })
                }
            }
            _ => Err(TypeError {
                message: "cannot assign to this expression".into(),
            }),
        }
    }

    fn check_block(&mut self, block: &Block) -> Result<Flow, TypeError> {
        self.env.push();
        let mut flow = Flow::Next;
        for stmt in &block.stmts {
            flow = self.check_stmt(stmt)?;
            if flow != Flow::Next {
                break;
            }
        }
        self.env.pop();
        Ok(flow)
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<Flow, TypeError> {
        match stmt {
            Stmt::Return(expr) => {
                let ty = self.check_expr(expr)?;
                if ty != self.return_ty {
                    return Err(TypeError {
                        message: format!(
                            "return type mismatch: expected `{}`, found `{}`",
                            self.return_ty.name(),
                            ty.name()
                        ),
                    });
                }
                Ok(Flow::Return)
            }
            Stmt::Print(expr) => {
                let ty = self.check_expr(expr)?;
                if ty != Ty::I32 {
                    return Err(TypeError {
                        message: format!("`print` requires `i32`, found `{}`", ty.name()),
                    });
                }
                Ok(Flow::Next)
            }
            Stmt::Let { name, ty, value } => {
                let value_ty = self.check_expr(value)?;
                let ty = match ty {
                    Some(ann) => {
                        let ann_ty = self.resolve(ann)?;
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
                self.env.declare(name, ty)?;
                Ok(Flow::Next)
            }
            Stmt::Assign { target, value } => {
                let var_ty = self.check_place(target)?;
                let value_ty = self.check_expr(value)?;
                if var_ty != value_ty {
                    return Err(TypeError {
                        message: format!(
                            "cannot assign `{}` to expression of type `{}`",
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
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Ty::Bool {
                    return Err(TypeError {
                        message: format!(
                            "`if` condition must be `bool`, found `{}`",
                            cond_ty.name()
                        ),
                    });
                }
                let then_flow = self.check_block(then_block)?;
                let else_flow = match else_block {
                    Some(else_block) => Some(self.check_block(else_block)?),
                    None => None,
                };
                Ok(join_if(then_flow, else_flow))
            }
            Stmt::While { cond, body } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Ty::Bool {
                    return Err(TypeError {
                        message: format!(
                            "`while` condition must be `bool`, found `{}`",
                            cond_ty.name()
                        ),
                    });
                }
                self.loop_depth += 1;
                let _ = self.check_block(body)?;
                self.loop_depth -= 1;
                Ok(Flow::Next)
            }
            Stmt::For {
                name,
                start,
                end,
                body,
            } => {
                let start_ty = self.check_expr(start)?;
                let end_ty = self.check_expr(end)?;
                if start_ty != Ty::I32 || end_ty != Ty::I32 {
                    return Err(TypeError {
                        message: format!(
                            "`for` range bounds must be `i32`, found `{}` and `{}`",
                            start_ty.name(),
                            end_ty.name()
                        ),
                    });
                }
                self.env.push();
                self.env.declare(name, Ty::I32)?;
                self.loop_depth += 1;
                let _ = self.check_block(body)?;
                self.loop_depth -= 1;
                self.env.pop();
                Ok(Flow::Next)
            }
            Stmt::Break => {
                if self.loop_depth == 0 {
                    return Err(TypeError {
                        message: "`break` outside of a loop".into(),
                    });
                }
                Ok(Flow::Jump)
            }
            Stmt::Continue => {
                if self.loop_depth == 0 {
                    return Err(TypeError {
                        message: "`continue` outside of a loop".into(),
                    });
                }
                Ok(Flow::Jump)
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Result<Ty, TypeError> {
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
            Expr::Var(name) => self.env.get(name).ok_or_else(|| TypeError {
                message: format!("undeclared variable `{name}`"),
            }),
            Expr::Call { name, args } => {
                let Some(sig) = self.fns.get(name) else {
                    return Err(TypeError {
                        message: format!("unknown function `{name}`"),
                    });
                };
                if args.len() != sig.params.len() {
                    return Err(TypeError {
                        message: format!(
                            "`{name}` expects {} argument{}, found {}",
                            sig.params.len(),
                            if sig.params.len() == 1 { "" } else { "s" },
                            args.len()
                        ),
                    });
                }
                for (i, arg) in args.iter().enumerate() {
                    let ty = self.check_expr(arg)?;
                    if ty != sig.params[i] {
                        return Err(TypeError {
                            message: format!(
                                "argument {} of `{name}`: expected `{}`, found `{}`",
                                i + 1,
                                sig.params[i].name(),
                                ty.name()
                            ),
                        });
                    }
                }
                Ok(sig.ret.clone())
            }
            Expr::StructLit { name, fields } => {
                let Some(def) = self.structs.get(name) else {
                    return Err(TypeError {
                        message: format!("unknown struct `{name}`"),
                    });
                };
                let mut seen = HashSet::new();
                for (field, value) in fields {
                    if !seen.insert(field.clone()) {
                        return Err(TypeError {
                            message: format!("duplicate field `{field}` in `{name}` literal"),
                        });
                    }
                    let Some(expected) = def.field(field) else {
                        return Err(TypeError {
                            message: format!("no field `{field}` on `{name}`"),
                        });
                    };
                    let ty = self.check_expr(value)?;
                    if ty != *expected {
                        return Err(TypeError {
                            message: format!(
                                "field `{field}` of `{name}`: expected `{}`, found `{}`",
                                expected.name(),
                                ty.name()
                            ),
                        });
                    }
                }
                for (field, _) in &def.fields {
                    if !seen.contains(field) {
                        return Err(TypeError {
                            message: format!("missing field `{field}` in `{name}` literal"),
                        });
                    }
                }
                Ok(Ty::Struct(name.clone()))
            }
            Expr::Field { base, field } => {
                let base_ty = self.check_expr(base)?;
                self.field_ty(&base_ty, field)
            }
            Expr::Unary { op, expr } => match op {
                UnOp::Neg => {
                    let ty = self.check_expr(expr)?;
                    if ty != Ty::I32 {
                        return Err(TypeError {
                            message: format!("`-` requires `i32`, found `{}`", ty.name()),
                        });
                    }
                    Ok(Ty::I32)
                }
                UnOp::Not => {
                    let ty = self.check_expr(expr)?;
                    if ty != Ty::Bool {
                        return Err(TypeError {
                            message: format!("`!` requires `bool`, found `{}`", ty.name()),
                        });
                    }
                    Ok(Ty::Bool)
                }
                UnOp::Deref => match self.check_expr(expr)? {
                    Ty::Ptr(inner) => Ok(*inner),
                    ty => Err(TypeError {
                        message: format!("`*` requires a pointer, found `{}`", ty.name()),
                    }),
                },
                UnOp::AddrOf => match self.check_place(expr) {
                    Ok(ty) => Ok(Ty::Ptr(Box::new(ty))),
                    Err(err) if err.message == "cannot assign to this expression" => {
                        Err(TypeError {
                            message: "cannot take address of this expression".into(),
                        })
                    }
                    Err(err) => Err(err),
                },
            },
            Expr::Binary { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs)?;
                let rhs_ty = self.check_expr(rhs)?;
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
                        if !matches!(lhs_ty, Ty::I32 | Ty::Bool) {
                            return Err(TypeError {
                                message: format!("cannot compare `{}`", lhs_ty.name()),
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
        assert!(
            check("fn main() -> i32 { if true && false || !false { return 1; } return 0; }")
                .is_ok()
        );
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

    #[test]
    fn accepts_call() {
        assert!(check(
            "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(1, 2); }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_recursive_call() {
        assert!(check(
            "fn sum(n: i32) -> i32 { if n <= 0 { return 0; } return n + sum(n - 1); } fn main() -> i32 { return sum(3); }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_unknown_function() {
        let err = check("fn main() -> i32 { return foo(1); }").unwrap_err();
        assert!(err.message.contains("unknown function"), "{}", err.message);
    }

    #[test]
    fn rejects_call_arity() {
        let err = check(
            "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(1); }",
        )
        .unwrap_err();
        assert!(err.message.contains("argument"), "{}", err.message);
    }

    #[test]
    fn rejects_call_arg_type() {
        let err = check(
            "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(true, 1); }",
        )
        .unwrap_err();
        assert!(err.message.contains("expected"), "{}", err.message);
    }

    #[test]
    fn rejects_duplicate_param() {
        let err = check("fn add(a: i32, a: i32) -> i32 { return a; }").unwrap_err();
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }

    #[test]
    fn accepts_struct_and_pointer() {
        assert!(check(
            "struct Point { x: i32, y: i32 } fn bump(p: *Point) -> i32 { p.x = p.x + 1; return p.x; } fn main() -> i32 { let p = Point { x: 1, y: 2 }; return bump(&p); }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_deref_i32_ptr() {
        assert!(check(
            "struct Point { x: i32 } fn main() -> i32 { let p = Point { x: 1 }; let q: *i32 = &p.x; *q = 4; return *q; }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_recursive_struct_via_pointer() {
        assert!(check(
            "struct Node { next: *Node, val: i32 } fn val(n: *Node) -> i32 { return n.val; } fn main() -> i32 { return 0; }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_unknown_field() {
        let err = check(
            "struct Point { x: i32 } fn main() -> i32 { let p = Point { x: 1 }; return p.y; }",
        )
        .unwrap_err();
        assert!(err.message.contains("no field"), "{}", err.message);
    }

    #[test]
    fn rejects_missing_struct_field() {
        let err = check(
            "struct Point { x: i32, y: i32 } fn main() -> i32 { let p = Point { x: 1 }; return p.x; }",
        )
        .unwrap_err();
        assert!(err.message.contains("missing field"), "{}", err.message);
    }

    #[test]
    fn rejects_recursive_struct_by_value() {
        let err = check("struct A { a: A } fn main() -> i32 { return 0; }").unwrap_err();
        assert!(err.message.contains("recursive"), "{}", err.message);
    }

    #[test]
    fn rejects_address_of_rvalue() {
        let err = check("fn main() -> i32 { let p: *i32 = &1; return 0; }").unwrap_err();
        assert!(err.message.contains("address"), "{}", err.message);
    }

    #[test]
    fn rejects_assign_to_temporary_field() {
        let err =
            check("struct Point { x: i32 } fn main() -> i32 { Point { x: 1 }.x = 2; return 0; }")
                .unwrap_err();
        assert!(err.message.contains("assign"), "{}", err.message);
    }
}
