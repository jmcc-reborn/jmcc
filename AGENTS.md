## MCP Servers: rust-analyzer, cargo, cratesio

Three MCP servers are available (`rust-analyzer`, `cargo`, `cratesio`). Use their tools directly through the MCP interface instead of guessing.

### Mandatory Rules

1. **Never guess a Rust type, trait bound, or signature.** Use `rust-analyzer` hover and definition queries before writing or modifying unfamiliar code, and query all references before renaming or deleting symbols.
2. **The first rust-analyzer answer in a session may come back empty** while the workspace is indexing. Wait briefly and retry if necessary.
3. **`jmcc-old/` is excluded from analysis on purpose.** It is a legacy copy and not a workspace member. Never read it, copy from it, or cite it as current behavior.
4. **`Hir` and `Mir` are `egg::define_language!` enums.** Their definitions are in `jmcc/src/ir/hir/mod.rs` and `jmcc/src/ir/mir/mod.rs`.
5. **Language source of truth:** The normative language specification is in [`SPECIFICATION.md`](SPECIFICATION.md) ([`SPECIFICATION_RU.md`](SPECIFICATION_RU.md)), developer guide in [`GUIDE.md`](GUIDE.md) ([`GUIDE_RU.md`](GUIDE_RU.md)), and standard library in `jmcc/std/**.jc`. Operators (`+`, `==`, `in`, `+=`, etc.) resolve to `__add__`, `__equals__`, `__contains__`, `__iadd__`, etc. on `@lang_item` classes in `jmcc/std/primitives/code/*.jc`. To change operator behavior, edit the `.jc` standard library, not compiler internals.
6. **Trust the compiler, not assumptions.** Claims about compiler pass operations or code generation must be backed by compiling a fixture and verifying emitted artifacts.
7. **No Implicit Fallbacks (Никогда не добавлять неявные фоллбеки).** Strive to NEVER add fallbacks that guess user intent or silently paper over invalid code, missing decorators, or misplaced imports. If code violates language semantics, is missing a required annotation (e.g. `@getter` / `@setter` on `__subscript__`), or uses an invalid path: emit a clear, actionable compile-time error with a suggestion (`did you mean '...'?`), never a silent fallback.
8. **JustMC Variable Default Values (Переменные по умолчанию):** В рантайме JustMC все переменные по умолчанию всегда равны нулю (`0`), поэтому чтение неинициализированной переменной не приводит к ошибкам времени выполнения.

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

Emitted JSON modules represent `Module { handlers: Vec<Line> }`. Handlers are sorted by `(Kind, BodyLength)`: events, functions, then processes. `CodeGen` wraps each handler body in `controller_measure_time` instrumentation when `debug_time` is enabled in the profile (`true` by default in `dev`, `false` by default in `release`). Top-level statements outside event blocks are grouped into a synthetic `world_start` event.

---

## Project Conventions

- **Documentation**: All documentation `.md` files default to English (`README.md`, `GUIDE.md`, `SPECIFICATION.md`), with paired Russian translations (`*_RU.md`).
- **Bilingual Language Support**: The compiler diagnostics and CLI support both English and Russian (`--locale ru|en`). The `.jc` language supports both English and Russian keywords.
- **Lints**: Workspace lints in `Cargo.toml` are strict. Use `#[expect(...)]` with an explicit reason when suppressing lints.

---

## Compiler Internals & Architecture Insights

### Dunder Dispatch, Operator Resolution & Annotations
- **Canonical Dunder Names**: Defined in `jmcc/src/ir/dunder.rs` (e.g. `__add__`, `__subtract__`, `__multiply__`, `__divide__`, `__remainder__`, `__pow__`, `__bitand__`, `__bitor__`, `__bitxor__`, `__lshift__`, `__rshift__`, `__equals__`, `__not_equals__`, `__contains__`, `__subscript__`, `__slice__`, `__init__`).
- **Operator `in` Semantics**: In `item in collection`, `item` is the element and `collection` is the container. Semantic analysis inverts the operands (`collection.__contains__(item)`). Represented in HIR as `Hir::In([item, coll])`. In `ir::hir_expand`, `Hir::In` lowers via `needs_overload(coll, "__contains__")` into `expand_inline_from_hir(&f, vec![coll, item])`.
- **Multiple `@alias` Arguments**: The `@alias` decorator supports multiple aliases in a single annotation line: `@alias("__contains__", "содержит")`.
- **Class Representation & Ranges (`Range`, `RangeInclusive`)**: User classes in JustMC / `jmcmock` are physically backed by arrays of field slots (`[field0, field1, ...]`). Half-open `start..end` and inclusive `start..=end` ranges lower into 4-slot instances `[start, end, current, step]`. To prevent `hir_expand` from mistakenly treating them as standard arrays, `conv_binary` binds the list to a temporary variable registered with `Type::Class(Range)` or `Type::Class(RangeInclusive)` in `ir_ctx.var_types`.

### Action Limit Splitting & Line Variable Liveness
- **Handler Splitting (`split_long_handlers`)**: JustMC enforces a hard limit of 50 actions per line (compiler default threshold is `MAX_ACTIONS_PER_LINE = 43`). Handlers exceeding this limit are automatically broken into chunks connected via `code::call_function("jmcc.N")`.
- **Bracket & Container Action Cost Nuance**: In JustMC physical action lines, container actions (`repeat`, `if`, `else`) take **2 actions by themselves** (1 for the opening container block + 1 for the closing bracket block), even when completely empty. Furthermore, each operation enclosed within the container's body resides on that same physical line, adding to the line length.
- **Line Context Preservation (Static Analysis)**: Because `call_function` creates an isolated frame (`stream.line`), line variables defined before the split would otherwise be lost. For regular variables, the compiler runs a liveness analysis (`DefinedBefore ∩ ReferencedInRemainder`), serializes surviving line variables into an arguments dictionary `Value::Map` (`TextValue` keys), and generates corresponding `Value::Parameter` headers in the target `jmcc.N` function to seamlessly restore the frame context.
- **Dynamic Line Variable Placeholders (`va_args`)**: When line variables containing placeholders (e.g. `%var_line(...)` or dynamic runtime names containing `%`) are defined before the split and referenced in the remainder, static analysis cannot resolve the runtime names into static `Value::Parameter`s. The compiler emits a warning and injects runtime dynamic argument packing and unpacking:
  - *Caller Packing*: calls `set_variable_get_list_variables(Line)` (1 action), initializes an empty `__va_args` map (1 action), iterates over line variables via `repeat_for_each_in_list` (container + closing bracket = 2 actions) inserting each variable into the map via `set_variable_set_map_value` (1 action), and invokes `code::call_function` (1 action).
  - *Line Budget Reservation*: Because the dynamic packing sequence consumes **6 actions** on the physical line (1 + 1 + 2 + 1 + 1 = 6), `walk_operations` reserves 6 slots (`reserved = 1 (call_function) + 5 (dynamic packing)`) whenever dynamic arguments are required, guaranteeing the handler line never exceeds `MAX_ACTIONS_PER_LINE = 43` (well below the hard 50-action limit).
  - *Callee Unpacking*: The continuation function `jmcc.N` declares `va_args: Map` in its parameters and prepends a `repeat_for_each_map_entry` loop at the very start of its body to unpack each dictionary entry back into line variables (`%var_line(__va_k) = __va_v`).

### Large Test Fixtures & Stack Sizing
- Compiling large test fixtures (such as `cubed.jc` with 345+ handlers or deep ASTs) requires a large stack. Tests running heavy compilations must be wrapped in `run_large_test(|| { ... })` with 32 MB stack allocation to prevent stack overflow (`SIGABRT`).

### Variable Scopes & Lifetimes Semantics
JustMC and `jmcmock` define four distinct variable scopes with strictly defined visibility boundaries and lifetimes:
- **`save` (Persistent Storage)**: Persists across server restarts. Values survive session terminations and are serialized to durable storage, preserving state between executions.
- **`game` / `global` (Global Execution Scope)**: Globally accessible from any location (any event, process, or function) throughout the entire runtime session. Cleared only on world reset.
- **`local` (Execution Root Scope)**: Scoped to the entire execution tree originating from a root entry point (such as an event handler or spawned process). Shared across all nested synchronous function call frames invoked within that branch, but completely isolated between different concurrent processes or distinct event triggers.
- **`line` (Call Frame Scope)**: The strictest scope with immediate frame isolation. Accessible only within the current call frame (`stream.line`). Discarded immediately upon frame return; invisible to caller frames and newly invoked callee frames (unless explicitly forwarded via parameters, as handled by `split_long_handlers`). Default scope in Edition 2026.
- **Access Complexity & No Scope Promotion**: Variable lookups and assignments across ALL scopes (`save`, `game`, `local`, `line`) run in $O(1)$ time in JustMC runtime — physically they are backed by the same hash map lookup mechanism. Synthetic "scope promotion" or variable caching (e.g. promoting `save` or `game` variables to temporary `line` variables) does not improve runtime performance and is harmful because it injects unnecessary copy actions into the action line.

### Declarative JSON Scenarios (`jmcmock`)
`jmcmock` supports declarative multi-action and multi-player simulations loaded from JSON files (`--scenario <FILE>` or `-s <FILE>`).
- **File Structure**: Either `{ "steps": [ ... ] }` or a raw JSON array `[ ... ]`.
- **Supported Step Types**:
  - `add_player`: `{ "type": "add_player", "name": "Steve", "x": 0.0, "y": 64.0, "z": 0.0 }`
  - `remove_player`: `{ "type": "remove_player", "name": "Steve" }`
  - `event`: `{ "type": "event", "name": "player_join", "player": "Steve", "args": [...] }` (when `"player"` is provided, the target player is resolved and bound to `Target::Player`).
  - `call_function`: `{ "type": "call_function", "name": "do_work", "args": [...] }`
  - `wait`: `{ "type": "wait", "ticks": 20 }` (advances the scheduler time queue).
  - `assert_log`: `{ "type": "assert_log", "contains": "joined" }` (validates mock log output; raises `RuntimeError::AssertionFailed` on mismatch).
  - `clear_log`: `{ "type": "clear_log" }` (flushes recorded log lines for clean step assertions).

### Mock Runtime World & Action Execution Model (`jmcmock`)
> [!CAUTION]
> **`jmcmock` реализован НЕ идеально — это лишь приблизительная модель рантайма JustMC!**
> `jmcmock` существует только для базовой офлайн-проверки и смоук-тестов. Он часто может работать некорректно или расходиться с реальным сервером: в нём не реализованы многие функции плейсхолдеров (например, `%math()`, `%length()`, `%index()`, `%entry()`), нет полной симуляции Minecraft AI, инвентарей, пакетов или серверных таймингов платформы. **Любое поведение, связанное с плейсхолдерами, строками, передачей данных или серверными механиками, ОБЯЗАТЕЛЬНО должно проверяться на реальном сервере JustMC!**

`jmcmock` implements execution semantics for core JustMC action groups without requiring a real Minecraft server:
- **World State Storage**:
  - **`Player`**: tracks `health`, `max_health`, `absorption_health`, `game_mode`, `food`, `saturation`, `experience`, `fire_ticks`, `is_flying`, `allow_flying`, `is_sneaking`, `is_sprinting`, `is_gliding`, `position`, `spawn_point`.
  - **`Entity`**: tracks `kind`, `name`, `health`, `max_health`, `absorption_health`, `fire_ticks`, `position`.
  - **`World`**: tracks block map `HashMap<(i64, i64, i64), String>`, `world_time`, `weather`, and event cancellation state.
- **Implemented World & Player Actions (`actions/effect/`)**:
  - **Player Actions**: `PlayerTeleport`, `PlayerRandomizedTeleport`, `PlayerSetHealth`, `PlayerSetMaxHealth`, `PlayerSetAbsorptionHealth`, `PlayerHeal`, `PlayerDamage`, `PlayerSetGamemode`, `PlayerSetVelocity`, `PlayerLaunchUp`, `PlayerLaunchForward`, `PlayerLaunchToLocation`, `PlayerGiveItems`, `PlayerGiveRandomItem`, `PlayerClearInventory`, `PlayerCloseInventory`, `PlayerSetFireTicks`, `PlayerSetFood`, `PlayerSetSaturation`, `PlayerSetExperience`, `PlayerGiveExperience`, `PlayerSetAllowFlying`, `PlayerSetFlying`, `PlayerKick`, `PlayerSetSpawnPoint`, `PlayerSetCompassTarget`, `PlayerSetTime`, `PlayerSetWeather`, `PlayerResetWeather`, `PlayerSendMessage`, `PlayerSendActionBar`, `PlayerSendTitle`, `PlayerSendMinimessage`, `PlayerSendHover`, `PlayerSendAdvancement`, `PlayerPlaySound`, `PlayerSetBossBar`, `PlayerRemoveBossBar`, `PlayerDisplayParticle`, `PlayerDisplayBlock`.
  - **Entity Actions**: `EntityTeleport`, `EntityDamage`, `EntityHeal`, `EntitySetCurrentHealth`, `EntitySetMaxHealth`, `EntitySetAbsorptionHealth`, `EntitySetFireTicks`, `EntityRemove`, `EntityExplode`, `EntityLaunchUp`, `EntityLaunchForward`, `EntityLaunchToLocation`.
  - **World Actions**: `GameSetBlock`, `GameBreakBlock`, `GameCreateExplosion`, `GameSpawnMob`, `GameSpawnArmorStand`, `GameSpawnItem`, `GameSpawnItemDisplay`, `GameSpawnBlockDisplay`, `GameSpawnTextDisplay`, `GameSpawnLightningBolt`, `GameLaunchFirework`, `GameSetWorldTime`, `GameSetWorldWeather`, `GameCancelEvent`, `GameUncancelEvent`, `GameSetEventDamage`, `GameSetEventHeal`, `GameSetEventExperience`, `GameSetEventGamemode`.
- **Extended Conditions & Game Values**:
  - Evaluates `if_game_event_is_canceled`, `if_player_gamemode_equals`, `if_player_name_equals`, `if_player_is_flying`, `if_player_is_sneaking`, `if_player_is_sprinting`, `if_player_is_gliding`, `if_game_block_equals`.
  - Exposes game values: `Location`, `CurrentHealth`, `MaxHealth`, `AbsorptionHealth` (for both players and entities), `Gamemode`, `FoodLevel`, `FoodSaturation`, `ExperienceLevel`, `FireTicks`, `WorldWeather`, `WorldTime`, `WorldGameTime`.

### Compound Assignment & Setter Mutation Semantics
- **Target Value Propagation**: In compound assignment lowering (`+=`, `-=`, `*=`, `/=`, etc.) for indexed (`arr[i] += val`) or property (`obj.prop += val`) access, the calculated operator node `op_id` must be passed as the setter value, not the original RHS operand `a.value`.
- **Value-Pass Mutation & Setter Auto-Return**: In JustMC's value-passing model, mutating an object via a setter creates a modified copy. When expanding inline setters with no explicit return type (`f.is_setter && f.return_type.is_none()`), the compiler automatically binds `ret_var = first_param_p_var` (`self`) so that the resulting modified instance is assigned back to the target object (`Set([tgt, ret_var])`).
- **Strict Accessor Decorators (No Method Fallbacks)**: Methods `__subscript__` and `__slice__` MUST be explicitly annotated with `@getter` (for reading `obj[i]`, `obj[i:j]`) or `@setter` (for assignment `obj[i] = v`, `obj[i:j] = v`). Without the required decorator, the compiler rejects the operation with error `E0039` / `E0038` and a hint pointing to the missing decorator, rather than falling back to regular methods.
- **Default Constructor Field Slot Mapping**: Classes without explicit `__init__` constructor declarations map positional and named instantiation arguments directly into their respective field slots (`slots[idx] = val_id`), falling back to `0.0` for omitted slots.
- **Strict Import Paths (No Search Fallbacks)**: Imports must explicitly declare their module origin (`"std/..."` for standard library modules, relative paths for local files, or package names for packages). The compiler does NOT silently fall back to probing `std/` for arbitrary relative imports; instead, it errors with a helpful suggestion if a matching standard library module exists.

### Single-Field Class Representation (Built-in Layout Optimization)
- **Direct Scalar Representation**: User classes (non-`@dict`, non-`@lang_item`, non-interface) having `total_fields_count == 1` are represented directly by their field value as a scalar, completely eliminating `create_list`, `get_list_value`, and `set_list_value` overhead.
- **Built-in Lowering vs Optimizer Pass**: This optimization is built directly into type lowering (`ast_to_hir` / `hir_expand`), mirroring how Rust handles 1-field structs (`Abi::Scalar` / `#[repr(transparent)]`) and Kotlin handles `value class`. It MUST NOT be deferred to a post-lowering optimizer pass: in untyped IR, distinguishing single-field class instances from 1-element user arrays (`[42]`) is unviable, and function call ABI boundaries would desynchronize without costly whole-program interprocedural analysis.

### IR Architectural Boundaries & JustMC Platform Model
- **JustMC Execution Model Reality**: JustMC (DiamondFire derivative) executes linear action lines (`Module { handlers: Vec<Line> }`) with nested bracketed action blocks (`operations` inside conditionals/loops). There are NO arbitrary CFG jump edges, basic blocks with phi-nodes, or registers. Never attempt to model JustMC IR as a generic CFG or LLVM-style SSA with jump edges.
- **HIR Design & Typing**:
  - *Type Storage*: Expression and variable types are tracked via symbol mappings in `ir_ctx.var_types`. Do NOT attach separate `HashMap<egg::Id, Type>` side-tables keyed by `egg::Id`: optimization passes (`math` e-graphs, inlining, DCE, copy propagation) continuously renumber, duplicate, and invalidate `egg::Id`s. Symbols in `ir_ctx.var_types` remain stable across transformations.
  - *Field Access via `Hir::Index`*: Class slot access should emit `Hir::Index([target, slot_id])` and `Hir::Set([Hir::Index([target, slot_id]), val])` rather than raw `Hir::Action("set_variable_get_list_value", ...)`. `lower_to_mir` already handles lowering `Hir::Index` for both reads and writes. Emitting `Hir::Index` preserves clean expression trees and enables HIR DCE (`dce::DeadCodeEliminationPass`) to eliminate unused field reads, because `Hir::Index` is marked pure while `Hir::Action` is treated as impure.
  - *No Spans in IR Nodes*: Do NOT place source `Span`s inside `egg::define_language!` enum variants. Spans break node equality, hash deduplication, and Common Subexpression Elimination (CSE) in e-graphs. User-facing diagnostics belong strictly in `ast::analyze`.
- **MIR Design & Platform Actions**:
  - *Target Context*: Action targets/selectors are preserved natively inside `Mir::Action(ids)` at slot `ids[2]` (`Mir::Sel`).
  - *Action Collapsing*: Optimization passes like `CoordinateFoldingPass` collapse sequences of platform actions (`set_coordinate` $\rightarrow$ `set_all_coordinates`) to stay within the 50-action line limit. They operate on platform actions, not literal struct values; do not introduce synthetic value nodes that disrupt action merging.
  - *Dead Code Elimination*: Dead code elimination belongs in HIR before actions are lowered; duplicating DCE into MIR is redundant overhead.

### Dynamic Placeholders, Handler Splitting & DCE Target Liveness
- **Dynamic Argument Activation (`split_long_handlers`)**: Dynamic variable passing via `va_args` MUST be triggered whenever static analysis cannot guarantee frame variable coverage across a split:
  - *Dynamic Writes*: If `defined_before` contains variables with placeholders (`name.contains('%')`), it may have populated any variable at runtime. If `remainder` references any line variable, dynamic arguments must be injected.
  - *Dynamic Reads*: If `remainder` references variables with placeholders, it may dynamically read any variable defined earlier. If `defined_before` defined any line variables, dynamic arguments must be injected.
  - *Condition*: `(def_has_dyn && !referenced_in_remainder.is_empty()) || (ref_has_dyn && !defined_before.is_empty())`. Requiring both simultaneously is a critical defect that silently drops variables.
- **Recursive Line Placeholder Extraction**: References to line variables can appear inside `Value::Text`, `Value::Map` keys (`TextValue`), `Value::Variable` names (including global/local/save variables whose dynamic names reference line variables, e.g. `g\`%var_line(key)\``), `Value::Item`, and `Value::Block`. Extraction must recurse through nested placeholders (e.g. `%var_line(%var_line(k))`) so all referenced inner variables are accounted for.
- **Aliased Temporary Replacement Across All Scopes**: When `var_aliases` tracks temporary variables redirected to other variables, `CodeGen::emit_text_content` must replace all four possible scope prefixes (`%var_line`, `%var_local`, `%var`, `%var_save`) after resolving alias chains, preventing unresolved temporary placeholders from leaking into Edition 2023 or non-line scoped strings.
- **DCE Dynamic Target Liveness**: In `dce::collect_read_vars`, target expressions of `Hir::VarDecl([target, val])` and `Hir::Set([target, val])` must still be inspected for string literals containing `%var` placeholders. Variables referenced inside dynamic assignment target names (e.g. `` `prefix_%var_line(k)` = 1 ``) are variable reads; omitting them causes DCE to incorrectly eliminate definitions of variables used solely within target placeholders.
- **Balanced Parenthesis Parsing in Placeholders**: Finding closing parentheses in `%var` or `%var_line` placeholders must track nested parenthesis depth rather than using naive `.find(')')`, ensuring nested placeholders (such as `%var_line(%var_line(k))` or `%math(...)`) are extracted without premature truncation.
- **JustMC Placeholder Syntax Rules (`/help placeholders`)**:
  - *Values, Symbols & Targets*: MUST have closing `%` (`%player%`, `%player_uuid%`, `%worlds%`, `%time%`, `%default%`, `%selected%`).
  - *Functions & Variable Lookups*: Concluded with `)` and MUST **NOT** have a closing `%` (`%var(name)`, `%var_line(name)`, `%var_local(name)`, `%var_save(name)`, `%length(...)`, `%index(...)`, `%entry(...)`, `%math(...)`). Writing `"%var_line(x)%"` leaves a trailing literal `%` in the string (`"VAL%"`) because the platform consumes only `%var_line(x)`.
  - *Nested Placeholders*: JustMC natively resolves nested call expressions (e.g. `%var_line(%var_line(test))` or `%var(%player%_key)`). Do not emit manual `$` substitutions or string `.replace()` hacks, which break under compiler interpolation.
  - *Scope Boundaries Across Calls & Processes*: Synchronous `code::call_function` isolates `line` scope (`stream.line`), but shares `local` scope across the call tree; targets and `game`/`save` are preserved. Asynchronous `code::start_process` and `code::wait` isolate both `line` and `local` scopes (only `game` and `save` cross process boundaries).
  - *Percent Collisions*: Standard escaping (`%%`) is not supported by JustMC. A percent sign next to text (e.g. `50% for %player%`) can cause placeholder parsing collisions on the server.
- **Aliased Temporary Scope Preservation (`CodeGen::emit_scoped_var`)**:
  - When `emit_scoped_var` encounters a variable wrapped in `Mir::Line`, `Mir::Local`, etc., it must NOT unconditionally overwrite the scope of aliased temporaries (`is_aliased`) or variables with known durable scopes (`VariableScope::Global`, `VariableScope::Save`).
  - Otherwise, compiler inlining of operators (such as `__equals__` which declares `line var __inl_first`) coerces global operands into `line` scope (`{"variable": "%player%_...", "scope": "line"}`), crashing execution on JustMC (`variable '...' does not exist in the 'line' scope`).

### Optimization Passes, Dynamic Variables & Scope Safety (`MIR`)
- **Copy Coalescing String Literal Safety**: `CopyCoalescingPass` operates on variable reference nodes in MIR, but does NOT rewrite string literals (`Mir::Str`). Any compiler temporary (such as inlined function parameters `__inl_`) that is referenced inside any `Mir::Str` node (e.g. `%var_line(__inl_param)`) via `collect_str_referenced_vars` MUST NOT be forwarded, redirected, or marked dead; otherwise string literals will reference eliminated, dead variables at runtime.
- **Dynamic Variable Placeholder Exclusion in MIR Passes**: Dynamic variable names containing `%` (e.g. `g\`%var(k)\``) are runtime-evaluated and MUST NOT be treated as static scalar variables:
  - In `SetVariableFoldingPass`: variables with `%` in their name cannot be merged into `set_values`, and expressions referencing any variables in the current group cannot be reordered.
  - In `CopyCoalescingPass`: variables whose names contain `%` must be excluded from `copies`, `aliases`, and `action_writes`.
  - In `CoordinateFoldingPass`: variables whose names contain `%` must not be grouped into merged coordinate actions.
- **Redundant Else Elimination Safety**: `RedundantElseEliminationPass` removes `else` branches that assign `0.0` to a variable outside loops, assuming the variable is 0 on entry. This assumption is ONLY valid for:
  1. `edition >= 2026` line scope (in Edition 2023, default scope is `local` and variables may persist across executions).
  2. Variables strictly in `Mir::Line` scope (NEVER `Mir::Game`, `Mir::Save`, or `Mir::Local`).
  3. Compiler-generated temporaries (`name.starts_with("__mt")`). User variables (e.g. `var x = 5; if c { x = 1 } else { x = 0 }`) may have prior values; deleting `else { x = 0 }` causes incorrect retention of previous values!
- **Edition-Aware Temporary Scoping**: Compiler temporaries generated during lowering must respect edition default scopes:
  - `MirLowerer::fresh_temp`: Emits `Mir::Line` for `edition >= 2026`, and `Mir::Local` for `edition < 2026`.
  - `OverloadExpander`: Emits `self.default_scope` (`Local` in Edition 2023, `Line` in Edition 2026) for inline function parameters, return variables, and argument atomization temporaries.
  - `CodeGen::fresh_temp`: Uses `self.default_scope` instead of hardcoding `Line`.
- **Trailing Nops in Redundant Return Elimination**: In `RedundantReturnEliminationPass`, blocks containing trailing `Mir::Nop` statements after `Mir::ReturnFunc` must be inspected using reverse search (`rposition(|&c| !matches!(..., Nop))`) so that redundant return instructions followed by non-semantic Nops are reliably eliminated.
- **Dead Store Elimination (DSE) & Placeholder Deactivation (`ir::opt::mir::dse`)**:
  - Eliminates dead overwrites to `Mir::Line` variables within a single block when the value expression is pure (`is_pure_mir`) and the variable is unconditionally overwritten before any intervening read, branch, or function call.
  - If any node in the MIR function tree contains `%` (`has_any_placeholders`), DSE is completely deactivated to prevent removing variables that might be read dynamically at runtime.
- **HIR Copy Propagation Target Invalidation (`get_target_var_name`)**:
  - In `ir::opt::hir::copy_prop`, mutations to indexed slots (`Hir::Set([Hir::Index([target, slot]), val])`) must resolve and invalidate the root `target` variable via `get_target_var_name`. Otherwise, container mutations inside loop bodies (e.g. iterator progress in `while it.has_next()`) fail to register in `written_vars`, causing `CopyPropagationPass` to incorrectly treat the object as loop-invariant and causing infinite loops.
- **JustMC Action Schema Parameter Conformity**:
  - In `ir::mir::exprs`, `set_variable_get_list_value` requires parameter name `"number"` (representing the 1-based index). Emitting `"index"` violates the schema and fails runtime execution in JustMC and `jmcmock`.
- **Unused Variable Semantic Warnings & Dynamic Placeholders (`W0003`)**:
  - `ast::analyze` tracks local variable declarations and reads. If a declared local variable has zero reads and does not begin with `_`, warning `W0003: UnusedVariable` is emitted.
  - If the enclosing function or callable contains string interpolations or `%` placeholders (`callable_has_dynamic`), `W0003` warnings are suppressed for that callable to avoid false positives on dynamically accessed variables.
- **Overwritten Assignment Semantic Warning (`W0004: OverwrittenAssignment`)**:
  - `ast::analyze` tracks sequential assignments to local variables in statement blocks via `analyze_block_statements`. If a variable is overwritten before being read, warning `W0004` is emitted.
  - Automatically suppressed if `callable_has_dynamic` is active, or if the variable name begins with `_` or contains `%`, or if the code belongs to `std`.
- **Loop Invariant Semantic Warning (`W0005: LoopInvariant`)**:
  - `ast::analyze` inspects `while` and `for` loop bodies via `check_loop_invariants`. If an assignment computes a pure value whose operands are never modified within the loop body, warning `W0005` is emitted suggesting hoisting the invariant computation outside.
  - Automatically suppressed if dynamic placeholders are present, or if the variable name begins with `_`, or if the code belongs to `std`.
- **Loop-Invariant Code Motion (LICM) Pass (`ir::opt::hir::licm`)**:
  - Automatically hoists pure assignments whose variables and expressions are not modified inside `Hir::While` loops to before the loop. Reduces runtime ticks in JustMC by computing invariants once instead of every iteration.
  - Completely deactivated when dynamic `%` placeholders are detected in the loop.
- **Common Subexpression Elimination (CSE) Pass (`ir::opt::hir::cse`)**:
  - Scans statement blocks for duplicate pure computations (including `Hir::Index` slot access and arithmetic) and hoists them into temporary variables (`__cse_N`), eliminating redundant evaluations.
- **Loop Unrolling Pass (`ir::opt::hir::loop_unroll`)**:
  - Identifies simple `while i < end` loops with constant step 1 and trip count $1 \le N \le 4$. Replaces the loop with $N$ copies of the body, substituting the iteration variable with constant values and eliminating loop header overhead in JustMC.
- **Tail Call Optimization (TCO) Pass (`ir::opt::hir::tco`)**:
  - Identifies functions ending with a self-recursive tail call (`return f(...)` or `f(...)`). Rewrites the function body into a flat `while (true)` loop with parameter updates (`continue`), eliminating `code::call_function` frame allocations and preventing call stack overflow.
- **Branch Folding & Empty Conditional Pruning (`ir::opt::mir::branch_folding`)**:
  - Identifies conditional actions (`if_...` or `if`) whose `then` and `else` branches are both empty or contain only `Mir::Nop`. Prunes them completely and strips `Mir::Nop` instructions from blocks in MIR.
- **Consecutive If Same Condition Warning (`W0006: ConsecutiveIfSameCondition`) & Branch Merging Pass (`ir::opt::mir::branch_merging`)**:
  - `ast::analyze` warns (`W0006`) when two consecutive `if` statements check the identical condition without modifying the condition variables in the first branch.
  - `BranchMergingPass` in MIR merges consecutive `if` statements with identical conditions into a single block, saving 2 physical JustMC actions (condition + closing bracket) per merged branch.
- **Unused Parameter Semantic Warning (`W0007: UnusedParameter`) & Dead Argument Elimination Pass (`ir::opt::hir::dead_arg_elim`)**:
  - `ast::analyze` warns (`W0007`) when a parameter in a non-exported callable is never referenced in its body, suggesting a `_` prefix if intentional.
  - `DeadArgumentEliminationPass` removes unused arguments from internal, non-exported functions and updates all call sites across the HIR module.
- **Dead Loop Semantic Warning (`W0008: DeadLoop`) & Dead Loop Elimination Pass (`ir::opt::hir::dead_loop`)**:
  - `ast::analyze` warns (`W0008`) when a loop has an empty body without side effects.
  - `DeadLoopEliminationPass` in HIR eliminates loops with known false conditions (`false`, `0.0`), empty bodies, or pure bodies whose assignments are never read outside the loop, replacing them with `Hir::Nop`.
- **Loop Invariant Branch Semantic Warning (`W0009: LoopInvariantBranch`) & Loop Unswitch Pass (`ir::opt::hir::loop_unswitch`)**:
  - `ast::analyze` warns (`W0009`) when a loop body contains a conditional statement whose condition is invariant across iterations.
  - `LoopUnswitchPass` in HIR lifts loop-invariant conditionals outside the loop: `while cond { if inv { A } else { B } }` becomes `if inv { while cond { A } } else { while cond { B } }`, evaluating the condition once instead of per iteration.
- **Code Sinking Pass (`ir::opt::hir::code_sinking`)**:
  - Moves pure variable declarations and assignments into the specific branch of a following `if` statement where the variable is actually used, avoiding evaluation overhead when taking alternate branches.
- **MIR Instruction Combination Module (`ir::opt::mir::instcombine`)**:
  - Reorganizes MIR local peephole and combination optimizations into a dedicated `instcombine` module: `SetVariableFoldingPass`, `CoordinateFoldingPass`, `ActionPeepholePass`, and `MirConstantFoldingPass`.
  - For backwards compatibility, legacy paths in `ir::opt::mir::{coordinate_folding, set_variable_folding}` re-export the implementations from `instcombine`.
- **Action Peephole Pass (`ir::opt::mir::instcombine::peephole`)**:
  - Specializes `variable::add(var, [var, K])` into `variable::increment(var, number = K)` and `variable::subtract(var, [var, K])` into `variable::decrement(var, number = K)` (when $K > 0$).
  - Merges adjacent `player::message` actions to the same receiver/selector with compatible merging options into a single action with combined `messages` array (up to the 18-element JustMC limit), saving 1 physical line action.
  - Merges adjacent `variable::append_list` and `variable::append_value` operations targeting the same variable, eliminating redundant container/array append actions.
- **MIR Constant Folding Pass (`ir::opt::mir::instcombine::constant_folding`)**:
  - Performs compile-time evaluation across all non-numeric and composite types in MIR:
    - *Text & Strings*: merges adjacent string literals in `Mir::Concat`, evaluates `join_text`, `get_text_length`, `trim_text`, and `replace_text`.
    - *Lists*: evaluates `get_list_value` by constant index, `get_list_length`, and `contains_value`.
    - *Maps*: performs compile-time lookup for `get_map_value` by constant key and computes `get_map_size`.
    - *Vectors & Locations*: extracts coordinates `get_vector_component` and `get_coordinate` from constant constructors `set_vector` / `set_all_coordinates`, and computes `get_vector_length`.
- **Loop Fusion Pass (`ir::opt::hir::loop_fusion`)**:
  - Merges adjacent `while` loops with identical induction variables and pure conditions into a single loop body when loop bounds are unmodified and data dependencies permit.
  - Merges adjacent `repeat::on_range` actions over identical ranges and `repeat::for_each_in_list` over identical lists into a single loop, saving 2 physical JustMC actions (container block + bracket) per fused loop.
