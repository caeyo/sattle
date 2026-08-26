# sattle

A SAT DSL for implementing CDCL solvers. Compiler: `sattlec`. Sources: `.satl`.

Requires LLVM 22 (`LLVM_SYS_221_PREFIX` set in `.cargo/config.toml`).

```bash
cargo run -p sattlec -- examples/add.satl              # emit LLVM IR (default)
cargo run -p sattlec -- --ast examples/add.satl        # dump AST
cargo run -p sattlec -- --tokens examples/add.satl     # dump tokens
cargo run -p sattlec -- --emit-llvm examples/add.satl  # emit LLVM IR
```
