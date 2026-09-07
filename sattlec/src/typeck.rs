//! Type checking.

use crate::ast::{BinOp, Block, Expr, Function, Item, MatchArm, Module, Stmt, Type, UnOp};
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
    Enum(String),
    Ptr(Box<Ty>),
}

impl Ty {
    pub fn name(&self) -> String {
        match self {
            Ty::I32 => "i32".into(),
            Ty::Bool => "bool".into(),
            Ty::Struct(name) | Ty::Enum(name) => name.clone(),
            Ty::Ptr(inner) => format!("*{}", inner.name()),
        }
    }
}

struct Binding {
    ty: Ty,
    mutable: bool,
}

struct Env {
    scopes: Vec<HashMap<String, Binding>>,
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

    fn declare(&mut self, name: &str, ty: Ty, mutable: bool) -> Result<(), TypeError> {
        let scope = self.scopes.last_mut().expect("typeck scope");
        if scope.contains_key(name) {
            return Err(TypeError {
                message: format!("duplicate variable `{name}`"),
            });
        }
        scope.insert(name.to_string(), Binding { ty, mutable });
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
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

struct EnumDef {
    variants: Vec<String>,
}

impl EnumDef {
    fn has_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|variant| variant == name)
    }
}

struct ConstDef {
    ty: Ty,
}

/// Type-check a module.
pub fn typeck(module: &Module) -> Result<(), TypeError> {
    let (structs, enums) = collect_types(module)?;
    let (fns, consts) = collect_values(module, &structs, &enums)?;

    let mut const_checker = checker(&fns, &structs, &enums, &consts, None);
    for item in &module.items {
        if let Item::Const(def) = item {
            check_const_ops(&def.value)?;
            let ty = const_checker.check_expr(&def.value)?;
            let expected = &consts[&def.name].ty;
            if ty != *expected {
                return Err(TypeError {
                    message: format!(
                        "const `{}` has type `{}` but initializer has type `{}`",
                        def.name,
                        expected.name(),
                        ty.name()
                    ),
                });
            }
        }
    }
    check_const_cycles(module, &consts)?;

    for item in &module.items {
        match item {
            Item::Fn(func) => {
                let mut chk = checker(
                    &fns,
                    &structs,
                    &enums,
                    &consts,
                    Some(fns[&func.name].ret.clone()),
                );
                chk.check_function(func)?;
            }
            Item::Struct(_) | Item::Enum(_) | Item::Const(_) => {}
        }
    }

    Ok(())
}

fn is_reserved_type(name: &str) -> bool {
    name == "i32" || name == "bool"
}

fn collect_types(
    module: &Module,
) -> Result<(HashMap<String, StructDef>, HashMap<String, EnumDef>), TypeError> {
    let mut structs = HashMap::new();
    let mut enums = HashMap::new();
    for item in &module.items {
        match item {
            Item::Struct(def) => {
                check_new_type(&def.name, &structs, &enums)?;
                structs.insert(def.name.clone(), StructDef { fields: Vec::new() });
            }
            Item::Enum(def) => {
                check_new_type(&def.name, &structs, &enums)?;
                let mut seen = HashSet::new();
                for variant in &def.variants {
                    if !seen.insert(variant.clone()) {
                        return Err(TypeError {
                            message: format!("duplicate variant `{variant}` on `{}`", def.name),
                        });
                    }
                }
                if def.variants.is_empty() {
                    return Err(TypeError {
                        message: format!("enum `{}` must have at least one variant", def.name),
                    });
                }
                enums.insert(
                    def.name.clone(),
                    EnumDef {
                        variants: def.variants.clone(),
                    },
                );
            }
            Item::Fn(_) | Item::Const(_) => {}
        }
    }

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
                fields.push((
                    field.name.clone(),
                    resolve_type(&field.ty, &structs, &enums)?,
                ));
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

    Ok((structs, enums))
}

fn check_new_type(
    name: &str,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Result<(), TypeError> {
    if is_reserved_type(name) {
        return Err(TypeError {
            message: format!("cannot define type with reserved name `{name}`"),
        });
    }
    if structs.contains_key(name) || enums.contains_key(name) {
        return Err(TypeError {
            message: format!("duplicate definition of `{name}`"),
        });
    }
    Ok(())
}

fn check_new_value(
    name: &str,
    fns: &HashMap<String, FnSig>,
    consts: &HashMap<String, ConstDef>,
) -> Result<(), TypeError> {
    if fns.contains_key(name) || consts.contains_key(name) {
        return Err(TypeError {
            message: format!("duplicate definition of `{name}`"),
        });
    }
    Ok(())
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

fn collect_values(
    module: &Module,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Result<(HashMap<String, FnSig>, HashMap<String, ConstDef>), TypeError> {
    let mut fns = HashMap::new();
    let mut consts = HashMap::new();
    for item in &module.items {
        match item {
            Item::Fn(func) => {
                check_new_value(&func.name, &fns, &consts)?;
                let mut params = Vec::new();
                for param in &func.params {
                    params.push(resolve_type(&param.ty, structs, enums)?);
                }
                let ret = resolve_type(&func.return_ty, structs, enums)?;
                fns.insert(func.name.clone(), FnSig { params, ret });
            }
            Item::Const(def) => {
                check_new_value(&def.name, &fns, &consts)?;
                consts.insert(
                    def.name.clone(),
                    ConstDef {
                        ty: resolve_type(&def.ty, structs, enums)?,
                    },
                );
            }
            Item::Struct(_) | Item::Enum(_) => {}
        }
    }
    Ok((fns, consts))
}

fn check_const_ops(expr: &Expr) -> Result<(), TypeError> {
    match expr {
        Expr::Call { name, .. } => Err(TypeError {
            message: format!("cannot call `{name}` in a const initializer"),
        }),
        Expr::Unary {
            op: UnOp::Deref, ..
        } => Err(TypeError {
            message: "cannot dereference in a const initializer".into(),
        }),
        Expr::Unary {
            op: UnOp::AddrOf, ..
        } => Err(TypeError {
            message: "cannot take address in a const initializer".into(),
        }),
        Expr::Unary { expr, .. } | Expr::Field { base: expr, .. } => check_const_ops(expr),
        Expr::Binary { lhs, rhs, .. } => {
            check_const_ops(lhs)?;
            check_const_ops(rhs)
        }
        Expr::StructLit { fields, .. } => {
            for (_, value) in fields {
                check_const_ops(value)?;
            }
            Ok(())
        }
        Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) | Expr::Variant { .. } => Ok(()),
    }
}

fn check_const_cycles(
    module: &Module,
    consts: &HashMap<String, ConstDef>,
) -> Result<(), TypeError> {
    let mut inits = HashMap::new();
    for item in &module.items {
        if let Item::Const(def) = item {
            inits.insert(def.name.as_str(), &def.value);
        }
    }
    let mut done = HashSet::new();
    let mut stack = Vec::new();
    for name in inits.keys().copied() {
        check_const_finite(name, &inits, consts, &mut stack, &mut done)?;
    }
    Ok(())
}

fn check_const_finite(
    name: &str,
    inits: &HashMap<&str, &Expr>,
    consts: &HashMap<String, ConstDef>,
    stack: &mut Vec<String>,
    done: &mut HashSet<String>,
) -> Result<(), TypeError> {
    if done.contains(name) {
        return Ok(());
    }
    if stack.iter().any(|seen| seen == name) {
        return Err(TypeError {
            message: format!("recursive const `{name}`"),
        });
    }
    stack.push(name.to_string());
    let mut deps = Vec::new();
    collect_const_refs(inits[name], consts, &mut deps);
    for dep in &deps {
        check_const_finite(dep, inits, consts, stack, done)?;
    }
    stack.pop();
    done.insert(name.to_string());
    Ok(())
}

fn collect_const_refs(expr: &Expr, consts: &HashMap<String, ConstDef>, deps: &mut Vec<String>) {
    match expr {
        Expr::Var(name) if consts.contains_key(name) => deps.push(name.clone()),
        Expr::Var(_) | Expr::Int(_) | Expr::Bool(_) | Expr::Variant { .. } => {}
        Expr::Unary { expr, .. } | Expr::Field { base: expr, .. } => {
            collect_const_refs(expr, consts, deps);
        }
        Expr::Binary { lhs, rhs, .. } => {
            collect_const_refs(lhs, consts, deps);
            collect_const_refs(rhs, consts, deps);
        }
        Expr::StructLit { fields, .. } => {
            for (_, value) in fields {
                collect_const_refs(value, consts, deps);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_const_refs(arg, consts, deps);
            }
        }
    }
}

fn resolve_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Result<Ty, TypeError> {
    match ty {
        Type::Name(name) if name == "i32" => Ok(Ty::I32),
        Type::Name(name) if name == "bool" => Ok(Ty::Bool),
        Type::Name(name) if structs.contains_key(name) => Ok(Ty::Struct(name.clone())),
        Type::Name(name) if enums.contains_key(name) => Ok(Ty::Enum(name.clone())),
        Type::Name(name) => Err(TypeError {
            message: format!("unknown type `{name}`"),
        }),
        Type::Ptr(inner) => Ok(Ty::Ptr(Box::new(resolve_type(inner, structs, enums)?))),
    }
}

struct Place {
    ty: Ty,
    mutable: bool,
    addressable: bool,
}

struct Checker<'a> {
    env: Env,
    fns: &'a HashMap<String, FnSig>,
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    consts: &'a HashMap<String, ConstDef>,
    return_ty: Option<Ty>,
    loop_depth: u32,
}

fn checker<'a>(
    fns: &'a HashMap<String, FnSig>,
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    consts: &'a HashMap<String, ConstDef>,
    return_ty: Option<Ty>,
) -> Checker<'a> {
    Checker {
        env: Env::new(),
        fns,
        structs,
        enums,
        consts,
        return_ty,
        loop_depth: 0,
    }
}

impl<'a> Checker<'a> {
    fn check_function(&mut self, func: &Function) -> Result<(), TypeError> {
        self.env.push();
        for param in &func.params {
            self.env.declare(
                &param.name,
                resolve_type(&param.ty, self.structs, self.enums)?,
                true,
            )?;
        }
        if self.check_block(&func.body)? != Flow::Return {
            return Err(TypeError {
                message: "missing `return`".into(),
            });
        }
        self.env.pop();
        Ok(())
    }

    fn lookup_value(&self, name: &str) -> Result<Ty, TypeError> {
        if let Some(binding) = self.env.get(name) {
            return Ok(binding.ty.clone());
        }
        if let Some(def) = self.consts.get(name) {
            return Ok(def.ty.clone());
        }
        Err(TypeError {
            message: format!("undeclared variable `{name}`"),
        })
    }

    fn resolve(&self, ty: &Type) -> Result<Ty, TypeError> {
        resolve_type(ty, self.structs, self.enums)
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

    fn check_place(&mut self, expr: &Expr) -> Result<Place, TypeError> {
        match expr {
            Expr::Var(name) => {
                if let Some(binding) = self.env.get(name) {
                    Ok(Place {
                        ty: binding.ty.clone(),
                        mutable: binding.mutable,
                        addressable: true,
                    })
                } else if let Some(def) = self.consts.get(name) {
                    Ok(Place {
                        ty: def.ty.clone(),
                        mutable: false,
                        addressable: false,
                    })
                } else {
                    Err(TypeError {
                        message: format!("undeclared variable `{name}`"),
                    })
                }
            }
            Expr::Unary {
                op: UnOp::Deref,
                expr,
            } => match self.check_expr(expr)? {
                Ty::Ptr(inner) => Ok(Place {
                    ty: *inner,
                    mutable: true,
                    addressable: true,
                }),
                ty => Err(TypeError {
                    message: format!("`*` requires a pointer, found `{}`", ty.name()),
                }),
            },
            Expr::Field { base, field } => {
                let base_ty = self.check_expr(base)?;
                let field_ty = self.field_ty(&base_ty, field)?;
                if matches!(base_ty, Ty::Ptr(_)) {
                    Ok(Place {
                        ty: field_ty,
                        mutable: true,
                        addressable: true,
                    })
                } else if let Ok(base) = self.check_place(base) {
                    Ok(Place {
                        ty: field_ty,
                        mutable: base.mutable,
                        addressable: base.addressable,
                    })
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
                let expected = self.return_ty.as_ref().expect("function");
                if ty != *expected {
                    return Err(TypeError {
                        message: format!(
                            "return type mismatch: expected `{}`, found `{}`",
                            expected.name(),
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
            Stmt::Let {
                name,
                ty,
                value,
                mutable,
            } => {
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
                self.env.declare(name, ty, *mutable)?;
                Ok(Flow::Next)
            }
            Stmt::Assign { target, value } => {
                let place = self.check_place(target)?;
                if !place.mutable {
                    return Err(TypeError {
                        message: "cannot assign to const".into(),
                    });
                }
                let value_ty = self.check_expr(value)?;
                if place.ty != value_ty {
                    return Err(TypeError {
                        message: format!(
                            "cannot assign `{}` to expression of type `{}`",
                            value_ty.name(),
                            place.ty.name()
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
                self.env.declare(name, Ty::I32, true)?;
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
            Stmt::Match { scrutinee, arms } => self.check_match(scrutinee, arms),
        }
    }

    fn check_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> Result<Flow, TypeError> {
        let scrut_ty = self.check_expr(scrutinee)?;
        let enum_name = enum_of(&scrut_ty)?;
        let variants = self.enums[enum_name].variants.clone();
        let mut seen = HashSet::new();
        let mut flows = Vec::new();
        for arm in arms {
            if arm.enum_name != enum_name {
                return Err(TypeError {
                    message: format!(
                        "pattern `{}::{}` does not match `{enum_name}`",
                        arm.enum_name, arm.variant
                    ),
                });
            }
            if !variants.iter().any(|variant| variant == &arm.variant) {
                return Err(TypeError {
                    message: format!("no variant `{}` on `{enum_name}`", arm.variant),
                });
            }
            if !seen.insert(arm.variant.clone()) {
                return Err(TypeError {
                    message: format!("duplicate match arm `{enum_name}::{}`", arm.variant),
                });
            }
            flows.push(self.check_block(&arm.body)?);
        }
        let missing: Vec<&String> = variants
            .iter()
            .filter(|variant| !seen.contains(*variant))
            .collect();
        if !missing.is_empty() {
            let list = missing
                .iter()
                .map(|variant| format!("`{enum_name}::{variant}`"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(TypeError {
                message: format!("non-exhaustive match on `{enum_name}`: missing {list}"),
            });
        }
        Ok(join_match(&flows))
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
            Expr::Var(name) => self.lookup_value(name),
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
            Expr::Variant { enum_name, variant } => {
                let Some(def) = self.enums.get(enum_name) else {
                    return Err(TypeError {
                        message: format!("unknown enum `{enum_name}`"),
                    });
                };
                if !def.has_variant(variant) {
                    return Err(TypeError {
                        message: format!("no variant `{variant}` on `{enum_name}`"),
                    });
                }
                Ok(Ty::Enum(enum_name.clone()))
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
                    Ok(place) => {
                        if !place.addressable {
                            return Err(TypeError {
                                message: "cannot take address of const".into(),
                            });
                        }
                        Ok(Ty::Ptr(Box::new(place.ty)))
                    }
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
                        if !matches!(lhs_ty, Ty::I32 | Ty::Bool | Ty::Enum(_)) {
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

fn join_match(flows: &[Flow]) -> Flow {
    if flows.iter().all(|flow| *flow == Flow::Return) {
        Flow::Return
    } else if flows.iter().any(|flow| *flow == Flow::Next) {
        Flow::Next
    } else {
        Flow::Jump
    }
}

fn enum_of(ty: &Ty) -> Result<&str, TypeError> {
    let mut cur = ty;
    loop {
        match cur {
            Ty::Enum(name) => return Ok(name),
            Ty::Ptr(inner) => cur = inner,
            _ => {
                return Err(TypeError {
                    message: format!("`match` requires an enum, found `{}`", ty.name()),
                });
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

    #[test]
    fn accepts_enum_and_match() {
        assert!(check(
            "enum Color { Red, Green } fn main() -> i32 { match Color::Red { Color::Red => { return 1; } Color::Green => { return 2; } } }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_match_on_enum_ptr() {
        assert!(check(
            "enum Color { Red, Green } fn main() -> i32 { let c = Color::Red; let p: *Color = &c; match p { Color::Red => { return 1; } Color::Green => { return 0; } } }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_enum_compare() {
        assert!(check(
            "enum Color { Red, Green } fn main() -> i32 { if Color::Red == Color::Green { return 1; } return 0; }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_non_exhaustive_match() {
        let err = check(
            "enum Color { Red, Green } fn main() -> i32 { match Color::Red { Color::Red => { return 1; } } }",
        )
        .unwrap_err();
        assert!(err.message.contains("non-exhaustive"), "{}", err.message);
        assert!(err.message.contains("Green"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_variant() {
        let err = check("enum Color { Red } fn main() -> i32 { return Color::Blue; }").unwrap_err();
        assert!(err.message.contains("no variant"), "{}", err.message);
    }

    #[test]
    fn rejects_duplicate_match_arm() {
        let err = check(
            "enum Color { Red, Green } fn main() -> i32 { match Color::Red { Color::Red => { return 1; } Color::Red => { return 2; } Color::Green => { return 3; } } }",
        )
        .unwrap_err();
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }

    #[test]
    fn rejects_match_missing_return() {
        let err = check(
            "enum Color { Red, Green } fn main() -> i32 { match Color::Red { Color::Red => { return 1; } Color::Green => { } } }",
        )
        .unwrap_err();
        assert!(err.message.contains("missing `return`"), "{}", err.message);
    }

    #[test]
    fn rejects_struct_enum_name_clash() {
        let err =
            check("struct Color { x: i32 } enum Color { Red } fn main() -> i32 { return 0; }")
                .unwrap_err();
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }

    #[test]
    fn accepts_item_and_local_const() {
        assert!(check("const N: i32 = 1 + 2; fn main() -> i32 { const m = N; return m; }").is_ok());
    }

    #[test]
    fn accepts_const_forward_ref() {
        assert!(
            check("const A: i32 = B; const B: i32 = 1; fn main() -> i32 { return A; }").is_ok()
        );
    }

    #[test]
    fn accepts_const_enum() {
        assert!(check(
            "enum Color { Red, Green } const START: Color = Color::Red; fn main() -> i32 { match START { Color::Red => { return 1; } Color::Green => { return 0; } } }"
        )
        .is_ok());
    }

    #[test]
    fn rejects_assign_to_local_const() {
        let err = check("fn main() -> i32 { const n = 1; n = 2; return n; }").unwrap_err();
        assert!(err.message.contains("const"), "{}", err.message);
    }

    #[test]
    fn rejects_assign_to_item_const() {
        let err = check("const N: i32 = 1; fn main() -> i32 { N = 2; return N; }").unwrap_err();
        assert!(err.message.contains("const"), "{}", err.message);
    }

    #[test]
    fn rejects_call_in_const() {
        let err =
            check("fn f() -> i32 { return 1; } const N: i32 = f(); fn main() -> i32 { return N; }")
                .unwrap_err();
        assert!(err.message.contains("call"), "{}", err.message);
    }

    #[test]
    fn rejects_recursive_const() {
        let err = check("const A: i32 = B; const B: i32 = A; fn main() -> i32 { return A; }")
            .unwrap_err();
        assert!(err.message.contains("recursive"), "{}", err.message);
    }

    #[test]
    fn rejects_const_fn_name_clash() {
        let err =
            check("const N: i32 = 1; fn N() -> i32 { return 0; } fn main() -> i32 { return 0; }")
                .unwrap_err();
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }
}
