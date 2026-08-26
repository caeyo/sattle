//! Abstract syntax tree.

use std::fmt;

/// A parsed source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Fn(Function),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub return_ty: Type,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Return(Expr),
    Print(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
        }
    }
}

/// Pretty-print the AST.
pub fn format_ast(module: &Module) -> String {
    let mut out = String::new();
    write_module(&mut out, module, 0);
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_module(out: &mut String, module: &Module, depth: usize) {
    indent(out, depth);
    out.push_str("Module\n");
    for item in &module.items {
        write_item(out, item, depth + 1);
    }
}

fn write_item(out: &mut String, item: &Item, depth: usize) {
    match item {
        Item::Fn(func) => write_function(out, func, depth),
    }
}

fn write_function(out: &mut String, func: &Function, depth: usize) {
    indent(out, depth);
    out.push_str(&format!("Fn {}\n", func.name));
    indent(out, depth + 1);
    out.push_str("ReturnType\n");
    write_type(out, &func.return_ty, depth + 2);
    indent(out, depth + 1);
    out.push_str("Body\n");
    write_block(out, &func.body, depth + 2);
}

fn write_type(out: &mut String, ty: &Type, depth: usize) {
    indent(out, depth);
    match ty {
        Type::Name(name) => out.push_str(&format!("Name({name})\n")),
    }
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    indent(out, depth);
    out.push_str("Block\n");
    for stmt in &block.stmts {
        write_stmt(out, stmt, depth + 1);
    }
}

fn write_stmt(out: &mut String, stmt: &Stmt, depth: usize) {
    match stmt {
        Stmt::Return(expr) => {
            indent(out, depth);
            out.push_str("Return\n");
            write_expr(out, expr, depth + 1);
        }
        Stmt::Print(expr) => {
            indent(out, depth);
            out.push_str("Print\n");
            write_expr(out, expr, depth + 1);
        }
    }
}

fn write_expr(out: &mut String, expr: &Expr, depth: usize) {
    indent(out, depth);
    match expr {
        Expr::Int(n) => out.push_str(&format!("Int({n})\n")),
        Expr::Binary { op, lhs, rhs } => {
            out.push_str(&format!("Binary({op})\n"));
            write_expr(out, lhs, depth + 1);
            write_expr(out, rhs, depth + 1);
        }
    }
}
