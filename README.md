# sattle

A SAT DSL for implementing CDCL solvers. Compiler: `sattlec` (Rust, LLVM). Sources: `.satl`.

```bash
cargo run -p sattlec -- examples/add.satl
```

Prints one token per line (`KIND @ line:col`, 1-based).
