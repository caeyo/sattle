# sattle

A SAT DSL for implementing CDCL solvers. Compiler: `sattlec`. Sources: `.satl`.

Requires LLVM 22 (`LLVM_SYS_221_PREFIX` set in `.cargo/config.toml`).

```bash
cargo run -p sattlec -- examples/add.satl               # native executable (a.out)
cargo run -p sattlec -- examples/add.satl -o add   # native executable
cargo run -p sattlec -- --emit-llvm examples/add.satl   # dump LLVM IR
cargo run -p sattlec -- --ast examples/add.satl         # dump AST
cargo run -p sattlec -- --tokens examples/add.satl      # dump tokens
```
