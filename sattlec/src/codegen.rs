//! LLVM code generation via inkwell.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt};
use crate::typeck::Ty;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::values::{FunctionValue, IntValue, PointerValue};
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use std::collections::HashMap;
use std::path::Path;

const PRINT_I32: &str = "sattle_print_i32";

/// A code-generation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
}

/// Emit LLVM IR for a type-checked module.
pub fn emit_llvm_ir(module: &Module, source_name: &str) -> Result<String, CodegenError> {
    with_llvm_module(module, source_name, |llvm_module| {
        Ok(llvm_module.print_to_string().to_string())
    })
}

/// Write a native object file for a type-checked module.
pub(crate) fn write_object(
    module: &Module,
    source_name: &str,
    obj_path: &Path,
) -> Result<(), CodegenError> {
    with_llvm_module(module, source_name, |llvm_module| {
        write_llvm_object(llvm_module, obj_path)
    })
}

fn with_llvm_module<T>(
    module: &Module,
    source_name: &str,
    f: impl FnOnce(&LlvmModule<'_>) -> Result<T, CodegenError>,
) -> Result<T, CodegenError> {
    let context = Context::create();
    let llvm_module = build_module(&context, module, source_name)?;
    f(&llvm_module)
}

fn write_llvm_object(llvm_module: &LlvmModule<'_>, obj_path: &Path) -> Result<(), CodegenError> {
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| CodegenError {
        message: format!("failed to initialize native LLVM target: {e}"),
    })?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| CodegenError {
        message: format!("failed to get target for {triple}: {e}"),
    })?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let machine = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError {
            message: format!("failed to create target machine for {triple}"),
        })?;

    llvm_module.set_triple(&triple);
    let data_layout = machine.get_target_data().get_data_layout();
    llvm_module.set_data_layout(&data_layout);

    if let Some(parent) = obj_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| CodegenError {
                message: format!("cannot create {}: {e}", parent.display()),
            })?;
        }
    }

    machine
        .write_to_file(llvm_module, FileType::Object, obj_path)
        .map_err(|e| CodegenError {
            message: format!("failed to write object {}: {e}", obj_path.display()),
        })
}

fn build_module<'ctx>(
    context: &'ctx Context,
    module: &Module,
    source_name: &str,
) -> Result<LlvmModule<'ctx>, CodegenError> {
    let llvm_module = context.create_module(source_name);
    llvm_module.set_source_file_name(source_name);
    declare_runtime(context, &llvm_module);
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

fn declare_runtime<'ctx>(context: &'ctx Context, llvm_module: &LlvmModule<'ctx>) {
    let void = context.void_type();
    let i32_ty = context.i32_type();
    let ty = void.fn_type(&[i32_ty.into()], false);
    llvm_module.add_function(PRINT_I32, ty, None);
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
    let mut cg = Codegen {
        context,
        llvm_module,
        builder,
        llvm_fn,
        scopes: Vec::new(),
    };
    cg.codegen_block(&func.body)?;
    if !cg.terminated() {
        return Err(CodegenError {
            message: "missing `return`".into(),
        });
    }
    Ok(())
}

struct Var<'ctx> {
    ptr: PointerValue<'ctx>,
    ty: Ty,
}

struct Codegen<'ctx, 'a> {
    context: &'ctx Context,
    llvm_module: &'a LlvmModule<'ctx>,
    builder: &'a Builder<'ctx>,
    llvm_fn: FunctionValue<'ctx>,
    scopes: Vec<HashMap<String, Var<'ctx>>>,
}

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    fn terminated(&self) -> bool {
        self.builder
            .get_insert_block()
            .and_then(BasicBlock::get_terminator)
            .is_some()
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, var: Var<'ctx>) {
        self.scopes
            .last_mut()
            .expect("codegen scope")
            .insert(name.to_string(), var);
    }

    fn lookup(&self, name: &str) -> Result<&Var<'ctx>, CodegenError> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .ok_or_else(|| CodegenError {
                message: format!("undeclared variable `{name}`"),
            })
    }

    fn llvm_ty(&self, ty: Ty) -> inkwell::types::IntType<'ctx> {
        match ty {
            Ty::I32 => self.context.i32_type(),
            Ty::Bool => self.context.bool_type(),
        }
    }

    fn alloca(&self, ty: Ty, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
        let current = self.builder.get_insert_block().ok_or_else(|| CodegenError {
            message: "alloca without insert block".into(),
        })?;
        let entry = self.llvm_fn.get_first_basic_block().ok_or_else(|| CodegenError {
            message: "function has no entry block".into(),
        })?;
        match entry.get_first_instruction() {
            Some(first) => self.builder.position_before(&first),
            None => self.builder.position_at_end(entry),
        }
        let ptr = self
            .builder
            .build_alloca(self.llvm_ty(ty), name)
            .map_err(|e| CodegenError {
                message: format!("failed to alloca `{name}`: {e}"),
            })?;
        self.builder.position_at_end(current);
        Ok(ptr)
    }

    fn load(&self, var: &Var<'ctx>, name: &str) -> Result<IntValue<'ctx>, CodegenError> {
        let value = self
            .builder
            .build_load(self.llvm_ty(var.ty), var.ptr, name)
            .map_err(|e| CodegenError {
                message: format!("failed to load `{name}`: {e}"),
            })?;
        Ok(value.into_int_value())
    }

    fn codegen_block(&mut self, block: &Block) -> Result<(), CodegenError> {
        self.push_scope();
        for stmt in &block.stmts {
            if self.terminated() {
                break;
            }
            self.codegen_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn codegen_stmt(&mut self, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::Return(expr) => {
                let value = self.codegen_expr(expr)?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(|e| CodegenError {
                        message: format!("failed to build return: {e}"),
                    })?;
                Ok(())
            }
            Stmt::Print(expr) => {
                let value = self.codegen_expr(expr)?;
                let print_fn = runtime_print_i32(self.llvm_module)?;
                self.builder
                    .build_call(print_fn, &[value.into()], "")
                    .map_err(|e| CodegenError {
                        message: format!("failed to build print: {e}"),
                    })?;
                Ok(())
            }
            Stmt::Let { name, value, .. } => {
                let init = self.codegen_expr(value)?;
                let ty = int_value_ty(self.context, init);
                let ptr = self.alloca(ty, name)?;
                self.builder.build_store(ptr, init).map_err(|e| CodegenError {
                    message: format!("failed to store `{name}`: {e}"),
                })?;
                self.declare(name, Var { ptr, ty });
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let val = self.codegen_expr(value)?;
                let ptr = self.lookup(name)?.ptr;
                self.builder.build_store(ptr, val).map_err(|e| CodegenError {
                    message: format!("failed to assign `{name}`: {e}"),
                })?;
                Ok(())
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
            } => self.codegen_if(cond, then_block, else_block.as_ref()),
            Stmt::While { cond, body } => self.codegen_while(cond, body),
        }
    }

    fn codegen_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<(), CodegenError> {
        let cond = self.codegen_expr(cond)?;
        let then_bb = self.context.append_basic_block(self.llvm_fn, "if.then");
        let else_bb = self.context.append_basic_block(self.llvm_fn, "if.else");
        self.builder
            .build_conditional_branch(cond, then_bb, else_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to build if: {e}"),
            })?;

        self.builder.position_at_end(then_bb);
        self.codegen_block(then_block)?;
        let then_term = self.terminated();
        let then_end = self.builder.get_insert_block().unwrap_or(then_bb);

        self.builder.position_at_end(else_bb);
        if let Some(else_block) = else_block {
            self.codegen_block(else_block)?;
        }
        let else_term = self.terminated();
        let else_end = self.builder.get_insert_block().unwrap_or(else_bb);

        if then_term && else_term {
            return Ok(());
        }

        let merge = self.context.append_basic_block(self.llvm_fn, "if.merge");
        if !then_term {
            self.builder.position_at_end(then_end);
            self.builder
                .build_unconditional_branch(merge)
                .map_err(|e| CodegenError {
                    message: format!("failed to leave then: {e}"),
                })?;
        }
        if !else_term {
            self.builder.position_at_end(else_end);
            self.builder
                .build_unconditional_branch(merge)
                .map_err(|e| CodegenError {
                    message: format!("failed to leave else: {e}"),
                })?;
        }
        self.builder.position_at_end(merge);
        Ok(())
    }

    fn codegen_while(&mut self, cond: &Expr, body: &Block) -> Result<(), CodegenError> {
        let cond_bb = self.context.append_basic_block(self.llvm_fn, "while.cond");
        let body_bb = self.context.append_basic_block(self.llvm_fn, "while.body");
        let after_bb = self.context.append_basic_block(self.llvm_fn, "while.after");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to enter while: {e}"),
            })?;

        self.builder.position_at_end(cond_bb);
        let cond = self.codegen_expr(cond)?;
        self.builder
            .build_conditional_branch(cond, body_bb, after_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to build while: {e}"),
            })?;

        self.builder.position_at_end(body_bb);
        self.codegen_block(body)?;
        if !self.terminated() {
            self.builder
                .build_unconditional_branch(cond_bb)
                .map_err(|e| CodegenError {
                    message: format!("failed to loop: {e}"),
                })?;
        }

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    fn codegen_expr(&self, expr: &Expr) -> Result<IntValue<'ctx>, CodegenError> {
        match expr {
            Expr::Int(value) => Ok(self.context.i32_type().const_int(*value as u64, true)),
            Expr::Bool(value) => Ok(self.context.bool_type().const_int(u64::from(*value), false)),
            Expr::Var(name) => {
                let var = self.lookup(name)?;
                self.load(var, name)
            }
            Expr::Binary { op, lhs, rhs } => {
                let lhs = self.codegen_expr(lhs)?;
                let rhs = self.codegen_expr(rhs)?;
                match op {
                    BinOp::Add => self.builder.build_int_add(lhs, rhs, "add").map_err(|e| {
                        CodegenError {
                            message: format!("failed to build add: {e}"),
                        }
                    }),
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.builder
                            .build_int_compare(int_pred(*op), lhs, rhs, "cmp")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build compare: {e}"),
                            })
                    }
                }
            }
        }
    }
}

fn int_pred(op: BinOp) -> IntPredicate {
    match op {
        BinOp::Eq => IntPredicate::EQ,
        BinOp::Ne => IntPredicate::NE,
        BinOp::Lt => IntPredicate::SLT,
        BinOp::Le => IntPredicate::SLE,
        BinOp::Gt => IntPredicate::SGT,
        BinOp::Ge => IntPredicate::SGE,
        BinOp::Add => unreachable!("add is not a compare"),
    }
}

fn int_value_ty<'ctx>(context: &'ctx Context, value: IntValue<'ctx>) -> Ty {
    if value.get_type() == context.bool_type() {
        Ty::Bool
    } else {
        Ty::I32
    }
}

fn runtime_print_i32<'ctx>(
    llvm_module: &LlvmModule<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    llvm_module.get_function(PRINT_I32).ok_or_else(|| CodegenError {
        message: format!("missing runtime declaration `{PRINT_I32}`"),
    })
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
    fn emit_llvm_declares_print() {
        let module = module_of("fn main() -> i32 { print(1 + 1); return 0; }");
        let ir = emit_llvm_ir(&module, "add.satl").unwrap();
        assert!(ir.contains(PRINT_I32), "{ir}");
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
