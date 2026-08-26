//! Compiler driver: object files → native executable.

use crate::ast::Module;
use crate::codegen::{write_object, CodegenError};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A driver error (linking, runtime lookup, wrapping codegen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverError {
    pub message: String,
}

impl From<CodegenError> for DriverError {
    fn from(err: CodegenError) -> Self {
        Self {
            message: err.message,
        }
    }
}

/// Compile a type-checked module to a native executable at `output`, linking `sattle_rt`.
pub fn compile_executable(
    module: &Module,
    source_name: &str,
    output: &Path,
) -> Result<(), DriverError> {
    let obj_path = object_path(output);
    write_object(module, source_name, &obj_path)?;
    let result = link_executable(&obj_path, output);
    let _ = std::fs::remove_file(&obj_path);
    result
}

fn object_path(output: &Path) -> PathBuf {
    let mut name = output
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("a"))
        .to_os_string();
    name.push(".o");
    match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn runtime_c_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sattle_rt")
        .join("sattle_rt.c")
}

fn link_executable(obj_path: &Path, output: &Path) -> Result<(), DriverError> {
    let rt = runtime_c_path();
    if !rt.exists() {
        return Err(DriverError {
            message: format!("sattle_rt not found at {}", rt.display()),
        });
    }

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let status = Command::new(&cc)
        .arg(obj_path)
        .arg(&rt)
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|e| DriverError {
            message: format!("failed to spawn `{cc}`: {e}"),
        })?;

    if !status.success() {
        return Err(DriverError {
            message: format!(
                "`{cc}` failed linking {} with {}",
                obj_path.display(),
                rt.display()
            ),
        });
    }
    Ok(())
}
