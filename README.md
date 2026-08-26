# sattle

A SAT DSL for implementing CDCL solvers. Compiler: `sattlec` (Rust, LLVM). Sources: `.satl`.

```bash
cargo run -p sattlec -- examples/add.satl          # dump AST (default)
cargo run -p sattlec -- --tokens examples/add.satl # dump tokens
cargo run -p sattlec -- --ast examples/add.satl    # dump AST
```
