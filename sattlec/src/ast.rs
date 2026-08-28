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
    Let {
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    Assign {
        name: String,
        value: Expr,
    },
    If {
        cond: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        cond: Expr,
        body: Block,
    },
    For {
        name: String,
        start: Expr,
        end: Expr,
        body: Block,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Var(String),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

impl fmt::Display for UnOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnOp::Neg => write!(f, "-"),
            UnOp::Not => write!(f, "!"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Rem => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
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
        Stmt::Let { name, ty, value } => {
            indent(out, depth);
            out.push_str(&format!("Let {name}\n"));
            if let Some(ty) = ty {
                indent(out, depth + 1);
                out.push_str("Type\n");
                write_type(out, ty, depth + 2);
            }
            write_expr(out, value, depth + 1);
        }
        Stmt::Assign { name, value } => {
            indent(out, depth);
            out.push_str(&format!("Assign {name}\n"));
            write_expr(out, value, depth + 1);
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
        } => {
            indent(out, depth);
            out.push_str("If\n");
            indent(out, depth + 1);
            out.push_str("Cond\n");
            write_expr(out, cond, depth + 2);
            indent(out, depth + 1);
            out.push_str("Then\n");
            write_block(out, then_block, depth + 2);
            if let Some(else_block) = else_block {
                indent(out, depth + 1);
                out.push_str("Else\n");
                write_block(out, else_block, depth + 2);
            }
        }
        Stmt::While { cond, body } => {
            indent(out, depth);
            out.push_str("While\n");
            indent(out, depth + 1);
            out.push_str("Cond\n");
            write_expr(out, cond, depth + 2);
            indent(out, depth + 1);
            out.push_str("Body\n");
            write_block(out, body, depth + 2);
        }
        Stmt::For {
            name,
            start,
            end,
            body,
        } => {
            indent(out, depth);
            out.push_str(&format!("For {name}\n"));
            indent(out, depth + 1);
            out.push_str("Start\n");
            write_expr(out, start, depth + 2);
            indent(out, depth + 1);
            out.push_str("End\n");
            write_expr(out, end, depth + 2);
            indent(out, depth + 1);
            out.push_str("Body\n");
            write_block(out, body, depth + 2);
        }
        Stmt::Break => {
            indent(out, depth);
            out.push_str("Break\n");
        }
        Stmt::Continue => {
            indent(out, depth);
            out.push_str("Continue\n");
        }
    }
}

fn write_expr(out: &mut String, expr: &Expr, depth: usize) {
    indent(out, depth);
    match expr {
        Expr::Int(n) => out.push_str(&format!("Int({n})\n")),
        Expr::Bool(b) => out.push_str(&format!("Bool({b})\n")),
        Expr::Var(name) => out.push_str(&format!("Var({name})\n")),
        Expr::Unary { op, expr } => {
            out.push_str(&format!("Unary({op})\n"));
            write_expr(out, expr, depth + 1);
        }
        Expr::Binary { op, lhs, rhs } => {
            out.push_str(&format!("Binary({op})\n"));
            write_expr(out, lhs, depth + 1);
            write_expr(out, rhs, depth + 1);
        }
    }
}
