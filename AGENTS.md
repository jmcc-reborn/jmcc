## MCP Servers: rust-analyzer, cargo, cratesio

Three MCP servers are available (`rust-analyzer`, `cargo`, `cratesio`). Use their tools directly through the MCP interface instead of guessing.

### Mandatory Rules

1. **Never guess a Rust type, trait bound, or signature.** Use `rust-analyzer` hover and definition queries before writing or modifying unfamiliar code, and query all references before renaming or deleting symbols.
2. **The first rust-analyzer answer in a session may come back empty** while the workspace is indexing. Wait briefly and retry if necessary.
3. **`jmcc-old/` is excluded from analysis on purpose.** It is a legacy copy and not a workspace member. Never read it, copy from it, or cite it as current behavior.
4. **`Hir` and `Mir` are `egg::define_language!` enums.** Their definitions are in `jmcc/src/ir/hir/mod.rs` and `jmcc/src/ir/mir/mod.rs`.
5. **Language source of truth:** The normative language specification is in [`SPECIFICATION.md`](SPECIFICATION.md) ([`SPECIFICATION_RU.md`](SPECIFICATION_RU.md)), developer guide in [`GUIDE.md`](GUIDE.md) ([`GUIDE_RU.md`](GUIDE_RU.md)), and standard library in `jmcc/std/**.jc`. Operators (`+`, `==`, `in`, `+=`, etc.) resolve to `__add__`, `__equals__`, `__contains__`, `__iadd__`, etc. on `@lang_item` classes in `jmcc/std/primitives/code/*.jc`. To change operator behavior, edit the `.jc` standard library, not compiler internals.
6. **Trust the compiler, not assumptions.** Claims about compiler pass operations or code generation must be backed by compiling a fixture and verifying emitted artifacts.

### Cargo Operations via MCP

Do not run `cargo check`, `cargo clippy`, `cargo test`, or `cargo fmt` in the shell when MCP tools are available. Call the corresponding MCP tools (`cargo_check`, `cargo_clippy`, `cargo_test`, `cargo_fmt_check`).

Shell `cargo` is reserved for running the compiler binary directly
(`RUST_LOG=warn cargo run -p jmcc -- compile <file.jc> --emit ast,hir,mir,json`), for `cargo generate-lockfile`, or for CLI flags not exposed by the MCP tools.

### Verification Loop

1. Run `cargo_check`, then `cargo_clippy` via MCP — ensure zero errors and zero warnings.
2. Run `cargo_test` via MCP — covers `jmcc` (lexer, i18n, diagnostics, formatting, project/lockfile, compilation), `jmcdata` (schema serialization), `jmcmock` (virtual runtime simulation), and `jmc-analyzer` (LSP and semantic tokens).
3. If compiler optimization or lowering behavior changed, compile relevant fixtures in `jmcc/tests/*.jc` and inspect the emitted HIR/MIR/JSON dumps to verify passes fired.

---

## Architecture Overview

JMCC ("JustMC Code Compiler") compiles JustCode (`.jc`) into the JSON program format consumed by the [JustMC](https://justmc.ru) Minecraft platform (DiamondFire derivative).

Cargo workspace with four members:
- `jmcc/` — Compiler binary (`src/`) and standard library (`std/`).
- `jmcdata/` — Offline model of JustMC actions, events, game values, selectors, and the output `Module`/`Op`/`Value` schema.
- `jmcmock/` — Mock runtime: loads compiled JSON modules and simulates execution without a Minecraft server.
- `jmc-analyzer/` — Language Server (LSP: diagnostics, completion, goto definition, hover, inlay hints, rename, references, formatting) and VS Code extension packager.

Pinned toolchain: `nightly` (`rust-toolchain.toml`), edition 2024.

---

## Commands

```bash
cargo build                      # Builds debug binaries
cargo run -p jmcc -- compile <file.jc>
cargo test                       # Runs test suite across all workspace crates (110+ tests)
cargo clippy                     # Strict workspace lints
```

### `compile` Command

```bash
jmcc compile <input.jc> [-o out.json] [-O 0..3] [--passes a,b] [--disable-passes c] \
              [--target justmc] [--edition 2026] [--emit ast,hir,mir,json]
```

- `-O`: Optimization level (default: `2`).
- `--passes` / `--disable-passes`: Case-insensitive pass names (enum variant name truncated at `(`, lowercased). Specifying `--passes` activates only listed passes.
- `--emit`: Default `ast,hir,mir,json`. Emits side artifacts: `<stem>.ast`, `<stem>_noopt.hir`, `<stem>_opt_preoverloads.hir`, `<stem>.hir`, `<stem>_noopt.mir`, `<stem>.mir`, `<stem>.json`.
- Logging: Handled via `tracing`. Default filter is `trace,egg=debug`; pipe through `RUST_LOG=warn` or `RUST_LOG=info` for readable output.

### `format` Command

`jmcc format <path> [--check]` runs the `.jc` code formatter on a file or directory tree.

---

## Compilation Pipeline

Authoritative ordering in `run_compile` (`jmcc/src/main.rs`):

1. **Panic Hook Installation**: `panic_handler::install_panic_hook()` captures backtraces and outputs `E0001` ICE reports on crash.
2. **Parsing & Import Resolution**: `ast::parse_file(path, edition)` parses and recursively merges imported `.jc` files into a single `Ast`.
3. **Lambda Lifting**: `ast::lift_lambdas(&mut ast)` extracts anonymous closures into synthetic global functions and callable classes.
4. **Symbol Table Building**: `IrCtx::new(&ast)` builds cross-phase symbol tables (classes, enums, type aliases, inline functions).
5. **Semantic Analysis**: `ast::analyze(&ast, &source, &ir_ctx, edition)` performs Hindley-Milner type inference with unification.
6. **HIR Lowering**: `ir::hir::ast_to_hir(...)` converts AST into High-Level IR (`RecExpr<Hir>`).
7. **HIR Optimizations**: Module passes (e.g. `inline`, `deadfunctionelimination`) and function passes (up to 3 fixpoint iterations).
8. **Overload Expansion**: `ir::hir_expand::expand_overloads(...)` expands operators into explicit dunder method calls on `@lang_item` classes.
9. **MIR Lowering**: `ir::mir::lower_to_mir(...)` lowers expanded HIR to Mid-Level IR (`RecExpr<Mir>`).
10. **MIR Optimizations**: Function passes (e.g. `redundantreturnelimination`, `copycoalescing`, `setvariablefolding`, `coordinatefolding`, `redundantelseelimination`).
11. **Code Generation**: `ir::codegen::CodeGen::generate(&mir)` constructs `jmcdata::module::Module` and serializes to JSON.

---

## The Two Intermediate Representations

Both `Hir` (`src/ir/hir/mod.rs`) and `Mir` (`src/ir/mir/mod.rs`) are `egg::define_language!` enums stored as `egg::RecExpr`.

- Almost everywhere, `egg` is used solely as an S-expression container (`Language::children()`, `map_children()`). Normal passes are plain `fn(&RecExpr<L>) -> RecExpr<L>`.
- The exception is `src/ir/opt/hir/math/`, which utilizes an e-graph `Runner` with equality saturation for constant folding and algebraic simplification.

---

## Language Editions (2023 vs 2026)

- **Edition 2026 (Default)**: Strict module encapsulation (`export` required to expose symbols), module name mangling, `line` variable scope by default, and strict type inference.
- **Edition 2023**: Wholesale AST merging, no mangling, `local` variable scope by default, dynamic `unknown` fallback for unresolved types.

---

## Output Structure

Emitted JSON modules represent `Module { handlers: Vec<Line> }`. Handlers are sorted by `(Kind, BodyLength)`: events, functions, then processes. `CodeGen` automatically wraps each handler body in `controller_measure_time` instrumentation. Top-level statements outside event blocks are grouped into a synthetic `world_start` event.

---

## Project Conventions

- **Documentation**: All documentation `.md` files default to English (`README.md`, `GUIDE.md`, `SPECIFICATION.md`), with paired Russian translations (`*_RU.md`).
- **Bilingual Language Support**: The compiler diagnostics and CLI support both English and Russian (`--locale ru|en`). The `.jc` language supports both English and Russian keywords.
- **Lints**: Workspace lints in `Cargo.toml` are strict. Use `#[expect(...)]` with an explicit reason when suppressing lints.
