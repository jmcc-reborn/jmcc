# jmcc

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

Compiler: transforms `.jc` source programs into JustMC JSON modules loaded by the server editor.

The module schema is defined in [`jmcdata`](../jmcdata/README.md), which supplies action names, events, and their parameter schemas. The emitted JSON module is executed by the [mock runtime](../jmcmock/README.md); `jmcc` does not execute code directly.

The compiler also defines language behavior. The language features fewer hardcoded keywords than expected: operators, literals, and standard action helpers are implemented in `.jc` within `std/` (see [`std/` is the language](#std-is-the-language)).

## Running

Two primary subcommands: `compile` and `format`.

```bash
RUST_LOG=warn cargo run -p jmcc -- compile jmcc/tests/case1.jc
RUST_LOG=warn cargo run -p jmcc -- format jmcc/tests/case2.jc
```

`RUST_LOG` is essential: the default tracing filter is `trace,egg=debug`, producing extensive trace logs. At `RUST_LOG=info`, enabled optimization passes and their execution timings are reported.

### `jmcc compile`

| Option | Description |
|---|---|
| `<input>` | `.jc` source file; imports and dumps are resolved relative to its directory |
| `-o, --output <PATH>` | Output JSON destination; defaults to adjacent file with identical stem |
| `-O, --opt-level <0..3>` | Optimization level, default `2` |
| `--passes <A,B>` | Comma-separated explicitly enabled optimization passes |
| `--disable-passes <A,B>` | Comma-separated disabled optimization passes |
| `--target <justmc>` | Compilation target (default: `justmc`) |
| `--edition <2023\|2026>` | Language edition, default `2026` |
| `--emit <ast,hir,mir,json>` | Files emitted to disk; defaults to all four |

`--emit` controls artifact emission. If `json` is omitted, code generation is skipped.

Exit codes: `0` for success and `1` for any compilation failure (unreadable file, parse error, semantic error, overload expansion error). Internal unexpected panics exit with code `101`.

### `jmcc format`

| Option | Description |
|---|---|
| `<input>` | `.jc` file or directory (recursively scans for `.jc` files) |
| `--check` | Dry run returning non-zero exit code if formatting discrepancies exist |

The formatter parses files with fixed `edition = 2026` without resolving imports, allowing standalone file formatting.

## Compilation Pipeline

The pipeline ordering in `run_compile` ([`main.rs`](src/main.rs)):

1. `ast::parse_file(path, edition)` ([`ast/import.rs`](src/ast/import.rs)) — Lexing, Pratt parsing, and recursive import resolution.
2. `ast::lift_lambdas` ([`ast/lambda_lift.rs`](src/ast/lambda_lift.rs)) — Lambda Lifting: lifts anonymous closures into synthetic functions and classes.
3. `IrCtx::new(&ast)` ([`ir/ctx.rs`](src/ir/ctx.rs)) — Shared symbol table construction.
4. `ast::analyze(&ast, &source, &ir_ctx, edition)` ([`ast/semantic/`](src/ast/semantic/)) — Hindley-Milner type inference, name resolution, scope validation, and action schemas.
5. `ir::hir::ast_to_hir(...)` ([`ir/hir/`](src/ir/hir/)) — AST lowering to High-Level IR.
6. `ir::opt::optimize(hir, …)` — HIR optimization passes.
7. `ir::hir_expand::expand_overloads(...)` ([`ir/hir_expand/`](src/ir/hir_expand/)) — Dunder method desugaring: operator expressions are transformed into method calls.
8. `ir::mir::lower_to_mir(&hir, &ir_ctx, edition)` ([`ir/mir/`](src/ir/mir/)) — Lowering to Mid-Level IR actions.
9. `ir::opt::optimize(mir, …)` — MIR optimization passes.
10. `ir::codegen::CodeGen::generate(&mir)` ([`ir/codegen/`](src/ir/codegen/)) — Constructs `jmcdata::module::Module` and serializes to JSON.

## Emitted Artifacts (Dumps)

| File | Contents |
|---|---|
| `<stem>.ast` | AST structure after parsing, prior to semantic analysis |
| `<stem>_noopt.hir` | Initial unoptimized HIR |
| `<stem>_opt_preoverloads.hir` | HIR after module/function optimizations, before overload expansion |
| `<stem>.hir` | HIR after overload expansion, lowered to MIR |
| `<stem>_noopt.mir` | MIR immediately after lowering |
| `<stem>.mir` | MIR after optimizations, passed to code generation |
| `<stem>.json` | Final JSON module (or output path from `-o`) |

`.ast` is emitted before semantic analysis and is generated even if compilation fails.

## The Two IRs (HIR and MIR)

Both IR representations are defined via `egg::define_language!` ([`ir/hir/mod.rs`](src/ir/hir/mod.rs), [`ir/mir/mod.rs`](src/ir/mir/mod.rs)) and stored in `RecExpr<Hir>` / `RecExpr<Mir>`.

In general, `egg` is utilized primarily as an S-expression container. E-graph rewriting is isolated to `src/ir/opt/hir/math/` for constant folding and algebraic simplification.

- **HIR**: High-level representation preserving classes, parameters, named arguments, and unexpanded operators.
- **MIR**: Low-level representation close to JustMC actions (`Mir::Action`), partitioned into containers, conditions, and explicit memory scopes.

```
AST                    HIR                     MIR                  JSON
a > b   ──analyze──▶  Gt(a, b)  ──expand──▶  Action(variable greater)  ──codegen──▶  op
```

## Optimizations

`ir::opt::optimize` ([`ir/opt/mod.rs`](src/ir/opt/mod.rs)) accepts the expression, registered passes, and `OptConfig { opt_level, enabled_passes, disabled_passes }`.

Pass naming for `--passes` corresponds to the lowercase enum variant name truncated at `(`.

| Stage | Pass Name | Opt Level | Description |
|---|---|---:|---|
| HIR Module | `testhirmodulepass` | 1 | Test pass |
| HIR Module | `inline` | 2 | Function inlining based on cost heuristics |
| HIR Module | `deadfunctionelimination` | 1 | Eliminates unreferenced functions |
| HIR Function | `noop` | 0 | No-op pass |
| HIR Function | `constantfolding` | 1 | E-graph constant evaluation |
| HIR Function | `algebraicsimplification` | 1 | Simplifies algebraic identities (`x + 0`, `x * 1`) |
| HIR Function | `copypropagation` | 1 | Propagates variable copies |
| HIR Function | `deadcodeelimination` | 1 | Eliminates unreachable blocks |
| MIR Module | `noop` | 0 | No-op pass |
| MIR Function | `redundantreturnelimination` | 1 | Removes trailing `return` operations |
| MIR Function | `copycoalescing` | 2 | Coalesces register copies |
| MIR Function | `setvariablefolding` | 2 | Merges sequential assignments |
| MIR Function | `coordinatefolding` | 2 | Folds coordinate calculations |
| MIR Function | `redundantelseelimination` | 2 | Eliminates empty `else` blocks |

## Editions

| Feature | Edition 2023 | Edition 2026 (Default) |
|---|---|---|
| Prelude | `std/prelude_2023.jc` | `std/prelude_2026.jc` |
| Mangling | Disabled | Enabled (`tests::generator::test`) |
| Imports | Merges all symbols | Gated by `export` |
| Default Scope | `local` | `line` |
| Process Parameters | Assigned via `set` prior to call | Lowered into `map` named `args` |

## `std/` is the Language

Operators in `.jc` resolve to methods on `@lang_item` classes in `std/primitives/code/`:

| Operator | Dunder Method | Operator | Dunder Method |
|---|---|---|---|
| `+` | `__add__` | `-=` | `__isubtract__` |
| `-` | `__subtract__` | `*=` | `__imultiply__` |
| `*` | `__multiply__` | `/=` | `__idivide__` |
| `/` | `__divide__` | `%=` | `__iremainder__` |
| `%` | `__remainder__` | `^=` | `__ipow__` |
| `^` | `__pow__` | `[]` (read) | `__subscript__` |
| `==` | `__equals__` | `[]` (write) | `__subscript__` with 3 params |
| `!=` | `__not_equals__` | `[a:b]` | `__slice__` |
| `>` | `__greater__` | `in` | `__contains__` (called on RHS) |
| `<` | `__less__` | `.` (read) | `__get_attribute__` |
| `>=` | `__greater_or_equals__` | `.` (write) | `__set_attribute__` |
| `<=` | `__less_or_equals__` | constructor | `__init__` |

## Building and Testing

```bash
cargo build -p jmcc
cargo clippy -p jmcc
cargo test -p jmcc
```

Automated tests in `jmcc` cover lexing, bilingual syntax, panic handling, project resolution, lockfile validation, and end-to-end compilation fixtures.
