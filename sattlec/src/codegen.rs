//! LLVM code generation via inkwell.

use crate::ast::{BinOp, Block, Expr, Function, Item, Module, Stmt, Type, UnOp};
use crate::typeck::Ty;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use std::collections::{HashMap, HashSet};
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
    let structs = declare_structs(context, module);

    let mut functions = HashMap::new();
    let mut saw_main = false;
    for item in &module.items {
        if let Item::Fn(func) = item {
            if func.name == "main" {
                saw_main = true;
            }
            let llvm_fn = declare_function(context, &llvm_module, &structs, func);
            functions.insert(
                func.name.clone(),
                FnInfo {
                    value: llvm_fn,
                    ret: ast_ty(&func.return_ty),
                },
            );
        }
    }

    if !saw_main {
        return Err(CodegenError {
            message: "`main` function not found".into(),
        });
    }

    for item in &module.items {
        if let Item::Fn(func) = item {
            codegen_function(context, &llvm_module, &builder, &functions, &structs, func)?;
        }
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

struct LlvmStruct<'ctx> {
    ty: StructType<'ctx>,
    fields: Vec<(String, Ty)>,
}

impl<'ctx> LlvmStruct<'ctx> {
    fn field(&self, name: &str) -> (u32, &Ty) {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, (field, _))| field == name)
            .map(|(i, (_, ty))| (i as u32, ty))
            .expect("typeck")
    }
}

struct FnInfo<'ctx> {
    value: FunctionValue<'ctx>,
    ret: Ty,
}

fn declare_structs<'ctx>(
    context: &'ctx Context,
    module: &Module,
) -> HashMap<String, LlvmStruct<'ctx>> {
    let mut structs = HashMap::new();
    for item in &module.items {
        if let Item::Struct(def) = item {
            let fields = def
                .fields
                .iter()
                .map(|field| (field.name.clone(), ast_ty(&field.ty)))
                .collect();
            structs.insert(
                def.name.clone(),
                LlvmStruct {
                    ty: context.opaque_struct_type(&def.name),
                    fields,
                },
            );
        }
    }
    define_struct_bodies(context, &structs);
    structs
}

fn define_struct_bodies<'ctx>(context: &'ctx Context, structs: &HashMap<String, LlvmStruct<'ctx>>) {
    let mut remaining: HashSet<String> = structs.keys().cloned().collect();
    while !remaining.is_empty() {
        let mut progress = false;
        let names: Vec<String> = remaining.iter().cloned().collect();
        for name in names {
            let ready = structs[&name].fields.iter().all(|(_, ty)| match ty {
                Ty::Struct(inner) => !remaining.contains(inner),
                _ => true,
            });
            if !ready {
                continue;
            }
            let field_tys: Vec<BasicTypeEnum> = structs[&name]
                .fields
                .iter()
                .map(|(_, ty)| llvm_ty(context, structs, ty))
                .collect();
            structs[&name].ty.set_body(&field_tys, false);
            remaining.remove(&name);
            progress = true;
        }
        assert!(progress, "typeck rejects recursive structs by value");
    }
}

fn declare_function<'ctx>(
    context: &'ctx Context,
    llvm_module: &LlvmModule<'ctx>,
    structs: &HashMap<String, LlvmStruct<'ctx>>,
    func: &Function,
) -> FunctionValue<'ctx> {
    let ret = ast_ty(&func.return_ty);
    let params: Vec<BasicMetadataTypeEnum> = func
        .params
        .iter()
        .map(|param| llvm_ty(context, structs, &ast_ty(&param.ty)).into())
        .collect();
    let fn_ty = llvm_ty(context, structs, &ret).fn_type(&params, false);
    let llvm_fn = llvm_module.add_function(&func.name, fn_ty, None);
    for (i, param) in func.params.iter().enumerate() {
        llvm_fn
            .get_nth_param(i as u32)
            .unwrap()
            .set_name(&param.name);
    }
    llvm_fn
}

fn codegen_function<'ctx>(
    context: &'ctx Context,
    llvm_module: &LlvmModule<'ctx>,
    builder: &Builder<'ctx>,
    functions: &HashMap<String, FnInfo<'ctx>>,
    structs: &HashMap<String, LlvmStruct<'ctx>>,
    func: &Function,
) -> Result<(), CodegenError> {
    let llvm_fn = functions[&func.name].value;
    let entry = context.append_basic_block(llvm_fn, "entry");
    builder.position_at_end(entry);
    let mut cg = Codegen {
        context,
        llvm_module,
        builder,
        llvm_fn,
        functions,
        structs,
        scopes: Vec::new(),
        loops: Vec::new(),
    };
    cg.push_scope();
    for (i, param) in func.params.iter().enumerate() {
        let ty = ast_ty(&param.ty);
        let val = llvm_fn.get_nth_param(i as u32).unwrap();
        let ptr = cg.alloca(&ty, &param.name)?;
        cg.builder.build_store(ptr, val).map_err(|e| CodegenError {
            message: format!("failed to store `{}`: {e}", param.name),
        })?;
        cg.declare(&param.name, Var { ptr, ty });
    }
    cg.codegen_block(&func.body)?;
    cg.pop_scope();
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

struct Loop<'ctx> {
    cont: BasicBlock<'ctx>,
    brk: BasicBlock<'ctx>,
}

struct Place<'ctx> {
    ptr: PointerValue<'ctx>,
    ty: Ty,
}

struct CVal<'ctx> {
    value: BasicValueEnum<'ctx>,
    ty: Ty,
}

impl<'ctx> CVal<'ctx> {
    fn int(self) -> IntValue<'ctx> {
        self.value.into_int_value()
    }
}

enum Operand<'ctx> {
    Place(Place<'ctx>),
    Value(CVal<'ctx>),
}

struct Codegen<'ctx, 'a> {
    context: &'ctx Context,
    llvm_module: &'a LlvmModule<'ctx>,
    builder: &'a Builder<'ctx>,
    llvm_fn: FunctionValue<'ctx>,
    functions: &'a HashMap<String, FnInfo<'ctx>>,
    structs: &'a HashMap<String, LlvmStruct<'ctx>>,
    scopes: Vec<HashMap<String, Var<'ctx>>>,
    loops: Vec<Loop<'ctx>>,
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

    fn ptr_ty(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.ptr_type(AddressSpace::default())
    }

    fn llvm_ty(&self, ty: &Ty) -> BasicTypeEnum<'ctx> {
        llvm_ty(self.context, self.structs, ty)
    }

    fn alloca(&self, ty: &Ty, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
        let current = self
            .builder
            .get_insert_block()
            .ok_or_else(|| CodegenError {
                message: "alloca without insert block".into(),
            })?;
        let entry = self
            .llvm_fn
            .get_first_basic_block()
            .ok_or_else(|| CodegenError {
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

    fn load_place(&self, place: Place<'ctx>) -> Result<CVal<'ctx>, CodegenError> {
        let value = self
            .builder
            .build_load(self.llvm_ty(&place.ty), place.ptr, "")
            .map_err(|e| CodegenError {
                message: format!("failed to load: {e}"),
            })?;
        Ok(CVal {
            value,
            ty: place.ty,
        })
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
                    .build_return(Some(&value.value))
                    .map_err(|e| CodegenError {
                        message: format!("failed to build return: {e}"),
                    })?;
                Ok(())
            }
            Stmt::Print(expr) => {
                let value = self.codegen_expr(expr)?.int();
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
                let ptr = self.alloca(&init.ty, name)?;
                self.builder
                    .build_store(ptr, init.value)
                    .map_err(|e| CodegenError {
                        message: format!("failed to store `{name}`: {e}"),
                    })?;
                self.declare(name, Var { ptr, ty: init.ty });
                Ok(())
            }
            Stmt::Assign { target, value } => {
                let place = self.codegen_place(target)?;
                let val = self.codegen_expr(value)?;
                self.builder
                    .build_store(place.ptr, val.value)
                    .map_err(|e| CodegenError {
                        message: format!("failed to assign: {e}"),
                    })?;
                Ok(())
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
            } => self.codegen_if(cond, then_block, else_block.as_ref()),
            Stmt::While { cond, body } => self.codegen_while(cond, body),
            Stmt::For {
                name,
                start,
                end,
                body,
            } => self.codegen_for(name, start, end, body),
            Stmt::Break => self.codegen_loop_jump(true),
            Stmt::Continue => self.codegen_loop_jump(false),
        }
    }

    fn codegen_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<(), CodegenError> {
        let cond = self.codegen_expr(cond)?.int();
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
        let cond = self.codegen_expr(cond)?.int();
        self.builder
            .build_conditional_branch(cond, body_bb, after_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to build while: {e}"),
            })?;

        self.builder.position_at_end(body_bb);
        self.loops.push(Loop {
            cont: cond_bb,
            brk: after_bb,
        });
        self.codegen_block(body)?;
        self.loops.pop();
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

    fn codegen_for(
        &mut self,
        name: &str,
        start: &Expr,
        end: &Expr,
        body: &Block,
    ) -> Result<(), CodegenError> {
        let start = self.codegen_expr(start)?.int();
        let end = self.codegen_expr(end)?.int();
        let i_ptr = self.alloca(&Ty::I32, name)?;
        let hi_ptr = self.alloca(&Ty::I32, "for.end")?;
        self.builder
            .build_store(i_ptr, start)
            .map_err(|e| CodegenError {
                message: format!("failed to store `{name}`: {e}"),
            })?;
        self.builder
            .build_store(hi_ptr, end)
            .map_err(|e| CodegenError {
                message: format!("failed to store for-range end: {e}"),
            })?;

        self.push_scope();
        self.declare(
            name,
            Var {
                ptr: i_ptr,
                ty: Ty::I32,
            },
        );

        let cond_bb = self.context.append_basic_block(self.llvm_fn, "for.cond");
        let body_bb = self.context.append_basic_block(self.llvm_fn, "for.body");
        let inc_bb = self.context.append_basic_block(self.llvm_fn, "for.inc");
        let after_bb = self.context.append_basic_block(self.llvm_fn, "for.after");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to enter for: {e}"),
            })?;

        self.builder.position_at_end(cond_bb);
        let i_val = self
            .builder
            .build_load(self.context.i32_type(), i_ptr, name)
            .map_err(|e| CodegenError {
                message: format!("failed to load `{name}`: {e}"),
            })?
            .into_int_value();
        let hi_val = self
            .builder
            .build_load(self.context.i32_type(), hi_ptr, "for.end")
            .map_err(|e| CodegenError {
                message: format!("failed to load for-range end: {e}"),
            })?
            .into_int_value();
        let cond = self
            .builder
            .build_int_compare(IntPredicate::SLT, i_val, hi_val, "for.cmp")
            .map_err(|e| CodegenError {
                message: format!("failed to build for: {e}"),
            })?;
        self.builder
            .build_conditional_branch(cond, body_bb, after_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to build for: {e}"),
            })?;

        self.builder.position_at_end(body_bb);
        self.loops.push(Loop {
            cont: inc_bb,
            brk: after_bb,
        });
        self.codegen_block(body)?;
        self.loops.pop();
        if !self.terminated() {
            self.builder
                .build_unconditional_branch(inc_bb)
                .map_err(|e| CodegenError {
                    message: format!("failed to leave for body: {e}"),
                })?;
        }

        self.builder.position_at_end(inc_bb);
        let i_val = self
            .builder
            .build_load(self.context.i32_type(), i_ptr, name)
            .map_err(|e| CodegenError {
                message: format!("failed to load `{name}`: {e}"),
            })?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(i_val, self.context.i32_type().const_int(1, true), "for.inc")
            .map_err(|e| CodegenError {
                message: format!("failed to increment `{name}`: {e}"),
            })?;
        self.builder
            .build_store(i_ptr, next)
            .map_err(|e| CodegenError {
                message: format!("failed to store `{name}`: {e}"),
            })?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to loop: {e}"),
            })?;

        self.builder.position_at_end(after_bb);
        self.pop_scope();
        Ok(())
    }

    fn codegen_loop_jump(&self, is_break: bool) -> Result<(), CodegenError> {
        let what = if is_break { "break" } else { "continue" };
        let dest = self.loops.last().unwrap();
        let dest = if is_break { dest.brk } else { dest.cont };
        self.builder
            .build_unconditional_branch(dest)
            .map_err(|e| CodegenError {
                message: format!("failed to build `{what}`: {e}"),
            })?;
        Ok(())
    }

    fn codegen_expr(&self, expr: &Expr) -> Result<CVal<'ctx>, CodegenError> {
        match self.codegen_operand(expr)? {
            Operand::Place(place) => self.load_place(place),
            Operand::Value(value) => Ok(value),
        }
    }

    fn codegen_place(&self, expr: &Expr) -> Result<Place<'ctx>, CodegenError> {
        match self.codegen_operand(expr)? {
            Operand::Place(place) => Ok(place),
            Operand::Value(_) => Err(CodegenError {
                message: "cannot assign to this expression".into(),
            }),
        }
    }

    fn codegen_operand(&self, expr: &Expr) -> Result<Operand<'ctx>, CodegenError> {
        match expr {
            Expr::Int(value) => Ok(Operand::Value(CVal {
                value: self
                    .context
                    .i32_type()
                    .const_int(*value as u64, true)
                    .into(),
                ty: Ty::I32,
            })),
            Expr::Bool(value) => Ok(Operand::Value(CVal {
                value: self
                    .context
                    .bool_type()
                    .const_int(u64::from(*value), false)
                    .into(),
                ty: Ty::Bool,
            })),
            Expr::Var(name) => {
                let var = self.lookup(name)?;
                Ok(Operand::Place(Place {
                    ptr: var.ptr,
                    ty: var.ty.clone(),
                }))
            }
            Expr::Call { name, args } => {
                let info = &self.functions[name];
                let mut compiled = Vec::new();
                for arg in args {
                    compiled.push(self.codegen_expr(arg)?.value.into());
                }
                let call = self
                    .builder
                    .build_call(info.value, &compiled, "call")
                    .map_err(|e| CodegenError {
                        message: format!("failed to call `{name}`: {e}"),
                    })?;
                Ok(Operand::Value(CVal {
                    value: call.try_as_basic_value().basic().unwrap(),
                    ty: info.ret.clone(),
                }))
            }
            Expr::StructLit { name, fields } => {
                Ok(Operand::Value(self.codegen_struct_lit(name, fields)?))
            }
            Expr::Field { base, field } => self.codegen_field(base, field),
            Expr::Unary { op, expr } => match op {
                UnOp::Neg => {
                    let value = self.codegen_expr(expr)?.int();
                    let neg =
                        self.builder
                            .build_int_neg(value, "neg")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build neg: {e}"),
                            })?;
                    Ok(Operand::Value(CVal {
                        value: neg.into(),
                        ty: Ty::I32,
                    }))
                }
                UnOp::Not => {
                    let value = self.codegen_expr(expr)?.int();
                    let not = self
                        .builder
                        .build_not(value, "not")
                        .map_err(|e| CodegenError {
                            message: format!("failed to build not: {e}"),
                        })?;
                    Ok(Operand::Value(CVal {
                        value: not.into(),
                        ty: Ty::Bool,
                    }))
                }
                UnOp::Deref => {
                    let value = self.codegen_expr(expr)?;
                    let Ty::Ptr(inner) = value.ty else {
                        unreachable!("typeck");
                    };
                    Ok(Operand::Place(Place {
                        ptr: value.value.into_pointer_value(),
                        ty: *inner,
                    }))
                }
                UnOp::AddrOf => {
                    let place = self.codegen_place(expr)?;
                    Ok(Operand::Value(CVal {
                        value: place.ptr.into(),
                        ty: Ty::Ptr(Box::new(place.ty)),
                    }))
                }
            },
            Expr::Binary {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                rhs,
            } => Ok(Operand::Value(CVal {
                value: self.codegen_short_circuit(*op, lhs, rhs)?.into(),
                ty: Ty::Bool,
            })),
            Expr::Binary { op, lhs, rhs } => {
                let lhs = self.codegen_expr(lhs)?.int();
                let rhs = self.codegen_expr(rhs)?.int();
                let (value, ty) = match op {
                    BinOp::Add => (
                        self.builder
                            .build_int_add(lhs, rhs, "add")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build add: {e}"),
                            })?,
                        Ty::I32,
                    ),
                    BinOp::Sub => (
                        self.builder
                            .build_int_sub(lhs, rhs, "sub")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build sub: {e}"),
                            })?,
                        Ty::I32,
                    ),
                    BinOp::Mul => (
                        self.builder
                            .build_int_mul(lhs, rhs, "mul")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build mul: {e}"),
                            })?,
                        Ty::I32,
                    ),
                    BinOp::Div => (
                        self.builder
                            .build_int_signed_div(lhs, rhs, "div")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build div: {e}"),
                            })?,
                        Ty::I32,
                    ),
                    BinOp::Rem => (
                        self.builder
                            .build_int_signed_rem(lhs, rhs, "rem")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build rem: {e}"),
                            })?,
                        Ty::I32,
                    ),
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (
                        self.builder
                            .build_int_compare(int_pred(*op), lhs, rhs, "cmp")
                            .map_err(|e| CodegenError {
                                message: format!("failed to build compare: {e}"),
                            })?,
                        Ty::Bool,
                    ),
                    BinOp::And | BinOp::Or => unreachable!("short-circuit ops are handled above"),
                };
                Ok(Operand::Value(CVal {
                    value: value.into(),
                    ty,
                }))
            }
        }
    }

    fn codegen_struct_lit(
        &self,
        name: &str,
        fields: &[(String, Expr)],
    ) -> Result<CVal<'ctx>, CodegenError> {
        let info = &self.structs[name];
        let mut agg = info.ty.get_undef();
        for (i, (field_name, _)) in info.fields.iter().enumerate() {
            let value = fields
                .iter()
                .find(|(n, _)| n == field_name)
                .map(|(_, expr)| expr)
                .expect("typeck");
            let value = self.codegen_expr(value)?;
            agg = self
                .builder
                .build_insert_value(agg, value.value, i as u32, field_name)
                .map_err(|e| CodegenError {
                    message: format!("failed to insert field `{field_name}`: {e}"),
                })?
                .into_struct_value();
        }
        Ok(CVal {
            value: agg.into(),
            ty: Ty::Struct(name.to_string()),
        })
    }

    fn codegen_field(&self, base: &Expr, field: &str) -> Result<Operand<'ctx>, CodegenError> {
        match self.codegen_operand(base)? {
            Operand::Place(place) => {
                let (ptr, name) = self.peel_place_to_struct(place)?;
                Ok(Operand::Place(self.field_place(ptr, &name, field)?))
            }
            Operand::Value(value) => match &value.ty {
                Ty::Struct(name) => {
                    let name = name.clone();
                    let (idx, field_ty) = self.structs[&name].field(field);
                    let extracted = self
                        .builder
                        .build_extract_value(value.value.into_struct_value(), idx, field)
                        .map_err(|e| CodegenError {
                            message: format!("failed to extract field `{field}`: {e}"),
                        })?;
                    Ok(Operand::Value(CVal {
                        value: extracted,
                        ty: field_ty.clone(),
                    }))
                }
                Ty::Ptr(_) => {
                    let (ptr, name) = self.peel_value_to_struct(value)?;
                    Ok(Operand::Place(self.field_place(ptr, &name, field)?))
                }
                Ty::I32 | Ty::Bool => unreachable!("typeck"),
            },
        }
    }

    fn field_place(
        &self,
        struct_ptr: PointerValue<'ctx>,
        struct_name: &str,
        field: &str,
    ) -> Result<Place<'ctx>, CodegenError> {
        let info = &self.structs[struct_name];
        let (idx, field_ty) = info.field(field);
        let ptr = self
            .builder
            .build_struct_gep(info.ty, struct_ptr, idx, field)
            .map_err(|e| CodegenError {
                message: format!("failed to gep field `{field}`: {e}"),
            })?;
        Ok(Place {
            ptr,
            ty: field_ty.clone(),
        })
    }

    fn peel_place_to_struct(
        &self,
        mut place: Place<'ctx>,
    ) -> Result<(PointerValue<'ctx>, String), CodegenError> {
        loop {
            match place.ty {
                Ty::Struct(name) => return Ok((place.ptr, name)),
                Ty::Ptr(inner) => {
                    let loaded = self
                        .builder
                        .build_load(self.ptr_ty(), place.ptr, "ldptr")
                        .map_err(|e| CodegenError {
                            message: format!("failed to load pointer: {e}"),
                        })?;
                    place = Place {
                        ptr: loaded.into_pointer_value(),
                        ty: *inner,
                    };
                }
                Ty::I32 | Ty::Bool => unreachable!("typeck"),
            }
        }
    }

    fn peel_value_to_struct(
        &self,
        mut value: CVal<'ctx>,
    ) -> Result<(PointerValue<'ctx>, String), CodegenError> {
        loop {
            match value.ty {
                Ty::Ptr(inner) => match *inner {
                    Ty::Struct(name) => return Ok((value.value.into_pointer_value(), name)),
                    other => {
                        let loaded = self
                            .builder
                            .build_load(self.ptr_ty(), value.value.into_pointer_value(), "ldptr")
                            .map_err(|e| CodegenError {
                                message: format!("failed to load pointer: {e}"),
                            })?;
                        value = CVal {
                            value: loaded,
                            ty: other,
                        };
                    }
                },
                Ty::I32 | Ty::Bool | Ty::Struct(_) => unreachable!("typeck"),
            }
        }
    }

    fn codegen_short_circuit(
        &self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<IntValue<'ctx>, CodegenError> {
        let (prefix, short_on_true) = match op {
            BinOp::And => ("and", false),
            BinOp::Or => ("or", true),
            _ => unreachable!("not a short-circuit op"),
        };

        let lhs_val = self.codegen_expr(lhs)?.int();
        let rhs_bb = self
            .context
            .append_basic_block(self.llvm_fn, &format!("{prefix}.rhs"));
        let short_bb = self
            .context
            .append_basic_block(self.llvm_fn, &format!("{prefix}.short"));
        let merge_bb = self
            .context
            .append_basic_block(self.llvm_fn, &format!("{prefix}.merge"));

        let (true_bb, false_bb) = if short_on_true {
            (short_bb, rhs_bb)
        } else {
            (rhs_bb, short_bb)
        };
        self.builder
            .build_conditional_branch(lhs_val, true_bb, false_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to build `{op}`: {e}"),
            })?;

        self.builder.position_at_end(rhs_bb);
        let rhs_val = self.codegen_expr(rhs)?.int();
        let rhs_end = self.builder.get_insert_block().unwrap_or(rhs_bb);
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to leave `{op}` rhs: {e}"),
            })?;

        self.builder.position_at_end(short_bb);
        let short_val = self
            .context
            .bool_type()
            .const_int(u64::from(short_on_true), false);
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| CodegenError {
                message: format!("failed to leave `{op}` short: {e}"),
            })?;

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(self.context.bool_type(), prefix)
            .map_err(|e| CodegenError {
                message: format!("failed to build `{op}` phi: {e}"),
            })?;
        phi.add_incoming(&[(&rhs_val, rhs_end), (&short_val, short_bb)]);
        Ok(phi.as_basic_value().into_int_value())
    }
}

fn llvm_ty<'ctx>(
    context: &'ctx Context,
    structs: &HashMap<String, LlvmStruct<'ctx>>,
    ty: &Ty,
) -> BasicTypeEnum<'ctx> {
    match ty {
        Ty::I32 => context.i32_type().into(),
        Ty::Bool => context.bool_type().into(),
        Ty::Struct(name) => structs[name].ty.into(),
        Ty::Ptr(_) => context.ptr_type(AddressSpace::default()).into(),
    }
}

fn ast_ty(ty: &Type) -> Ty {
    match ty {
        Type::Name(name) if name == "i32" => Ty::I32,
        Type::Name(name) if name == "bool" => Ty::Bool,
        Type::Name(name) => Ty::Struct(name.clone()),
        Type::Ptr(inner) => Ty::Ptr(Box::new(ast_ty(inner))),
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
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::And | BinOp::Or => {
            unreachable!("not a compare")
        }
    }
}

fn runtime_print_i32<'ctx>(
    llvm_module: &LlvmModule<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    llvm_module
        .get_function(PRINT_I32)
        .ok_or_else(|| CodegenError {
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
    fn emit_llvm_includes_callee() {
        let module = module_of(
            "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(1, 2); }",
        );
        let ir = emit_llvm_ir(&module, "call.satl").unwrap();
        assert!(ir.contains("define i32 @add"), "{ir}");
        assert!(ir.contains("call i32 @add"), "{ir}");
    }

    #[test]
    fn emit_llvm_short_circuits_and() {
        let module = module_of("fn main() -> i32 { if false && true { return 1; } return 0; }");
        let ir = emit_llvm_ir(&module, "logic.satl").unwrap();
        assert!(ir.contains("and.rhs"), "{ir}");
        assert!(ir.contains("and.merge"), "{ir}");
        assert!(ir.contains("phi"), "{ir}");
    }

    #[test]
    fn emit_llvm_struct_and_pointer() {
        let module = module_of(
            "struct Point { x: i32, y: i32 } fn bump(p: *Point) -> i32 { p.x = p.x + 1; return p.x; } fn main() -> i32 { let p = Point { x: 1, y: 2 }; return bump(&p); }",
        );
        let ir = emit_llvm_ir(&module, "struct.satl").unwrap();
        assert!(ir.contains("%Point"), "{ir}");
        assert!(ir.contains("define i32 @bump"), "{ir}");
        assert!(ir.contains("getelementptr"), "{ir}");
    }
}
