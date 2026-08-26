//! LLVM code generation via inkwell.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt};
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::IntValue;

/// A code-generation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
}

/// Emit LLVM IR for a type-checked module.
pub fn emit_llvm_ir(module: &Module, source_name: &str) -> Result<String, CodegenError> {
    let context = Context::create();
    let llvm_module = build_module(&context, module, source_name)?;
    Ok(llvm_module.print_to_string().to_string())
}

fn build_module<'ctx>(
    context: &'ctx Context,
    module: &Module,
    source_name: &str,
) -> Result<LlvmModule<'ctx>, CodegenError> {
    let llvm_module = context.create_module(source_name);
    llvm_module.set_source_file_name(source_name);
    let builder = context.create_builder();
    let mut saw_main = false;

    for item in &module.items {
        match item {
            Item::Fn(func) if func.name == "main" => {
                codegen_main(context, &llvm_module, &builder, func)?;
                saw_main = true;
            }
            Item::Fn(func) => {
                return Err(CodegenError {
                    message: format!("codegen: unsupported function `{}`", func.name),
                });
            }
        }
    }

    if !saw_main {
        return Err(CodegenError {
            message: "`main` function not found".into(),
        });
    }

    llvm_module.verify().map_err(|e| CodegenError {
        message: format!("LLVM module verification failed: {e}"),
    })?;

    Ok(llvm_module)
}

fn codegen_main<'ctx>(
    context: &'ctx Context,
    llvm_module: &LlvmModule<'ctx>,
    builder: &Builder<'ctx>,
    func: &Function,
) -> Result<(), CodegenError> {
    let i32_ty = context.i32_type();
    let fn_ty = i32_ty.fn_type(&[], false);
    let llvm_fn = llvm_module.add_function("main", fn_ty, None);
    let entry = context.append_basic_block(llvm_fn, "entry");
    builder.position_at_end(entry);
    codegen_block(context, builder, &func.body)?;
    Ok(())
}

fn codegen_block<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    block: &Block,
) -> Result<(), CodegenError> {
    for stmt in &block.stmts {
        codegen_stmt(context, builder, stmt)?;
    }
    Ok(())
}

fn codegen_stmt<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    stmt: &Stmt,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Return(expr) => {
            let value = codegen_expr(context, builder, expr)?;
            builder
                .build_return(Some(&value))
                .map_err(|e| CodegenError {
                    message: format!("failed to build return: {e}"),
                })?;
            Ok(())
        }
    }
}

fn codegen_expr<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    expr: &Expr,
) -> Result<IntValue<'ctx>, CodegenError> {
    let i32_ty = context.i32_type();
    match expr {
        Expr::Int(value) => Ok(i32_ty.const_int(*value as u64, true)),
        Expr::Binary { op, lhs, rhs } => {
            let lhs = codegen_expr(context, builder, lhs)?;
            let rhs = codegen_expr(context, builder, rhs)?;
            match op {
                BinOp::Add => builder
                    .build_int_add(lhs, rhs, "addtmp")
                    .map_err(|e| CodegenError {
                        message: format!("failed to build add: {e}"),
                    }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;
    use crate::typeck::typeck;

    fn module_of(src: &str) -> Module {
        let tokens = lex(src).unwrap();
        let module = parse(&tokens, src.len()).unwrap();
        typeck(&module).unwrap();
        module
    }

    #[test]
    fn emit_llvm_contains_main() {
        let module = module_of("fn main() -> i32 { return 1 + 1; }");
        let ir = emit_llvm_ir(&module, "add.satl").unwrap();
        assert!(ir.contains("define"), "{ir}");
        assert!(ir.contains("i32"), "{ir}");
        assert!(ir.contains("main"), "{ir}");
        assert!(ir.contains("source_filename = \"add.satl\""), "{ir}");
    }

    #[test]
    fn rejects_missing_main() {
        let module = module_of("");
        let err = emit_llvm_ir(&module, "empty.satl").unwrap_err();
        assert!(err.message.contains("main"), "{}", err.message);
    }

    #[test]
    fn rejects_non_main_function() {
        let module = module_of("fn add() -> i32 { return 1; }");
        let err = emit_llvm_ir(&module, "add.satl").unwrap_err();
        assert!(err.message.contains("add"), "{}", err.message);
    }
}
