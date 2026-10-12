# Changelog

All notable changes to the JustMC Code Compiler (`jmcc`) project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.0] - 2026-10-12

### Optimization & Instruction Combining
- **MIR Instruction Combining Pipeline (`instcombine`)**: Reorganized MIR transformations into an instruction combination pipeline integrating `ActionPeepholePass`, `SetVariableFoldingPass`, `CoordinateFoldingPass`, and `MirConstantFoldingPass`.
- **Action Peephole Pass (`ActionPeepholePass`)**:
  - Specializes general arithmetic into native JustMC counter actions (`variable::add` -> `set_variable_increment`, `variable::subtract` -> `set_variable_decrement`).
  - Merges adjacent `player::message` calls targeting the same recipient into an aggregated message array, saving physical line actions.
  - Merges adjacent `variable::append_list` and `variable::append_value` mutations.
- **MIR Constant Evaluation (`MirConstantFoldingPass`)**:
  - Text & Strings: merges contiguous string literals in `Concat`; folds constant calls to `join_text`, `get_text_length`, `trim_text`, and `replace_text`.
  - Collections: evaluates constant index lookups in `get_list_value`, `get_list_length`, `contains_value`, `get_map_value`, and `get_map_size`.
  - Spatial Data: folds `get_vector_component`, `get_coordinate`, and `get_vector_length` on static vector and location declarations.
- **Loop Optimizations (HIR)**:
  - **Loop Fusion (`LoopFusionPass`)**: Merges consecutive loops sharing identical bounds and unmodified induction variables, eliminating container and bracket overhead (2 actions per fused loop).
  - **Loop-Invariant Code Motion (`LoopInvariantCodeMotionPass`)**: Hoists pure loop-invariant computations out of `Hir::While` loop headers.
  - **Loop Unrolling (`LoopUnrollingPass`)**: Unrolls constant-bounded loops ($1 \le N \le 4$) with step 1 into sequential statements.
  - **Loop Unswitching (`LoopUnswitchPass`)**: Hoists loop-invariant conditional branches out of loop bodies.
  - **Dead Loop Elimination (`DeadLoopEliminationPass`)**: Removes loops with constant-false conditions, empty bodies, or dead side-effect-free computations.
- **Dataflow & Control Flow Passes**:
  - **Common Subexpression Elimination (`CommonSubexpressionEliminationPass`)**: Binds repeated pure subexpressions (including `Hir::Index` slot reads and arithmetic) into reuse variables (`__cse_N`).
  - **Dead Store Elimination (`DeadStoreEliminationPass`)**: Eliminates redundant overwritten assignments to line and local variables prior to reads, branches, or calls; strictly disabled when dynamic placeholder patterns (`%`) are detected.
  - **Tail Call Optimization (`TailCallOptimizationPass`)**: Transforms self-tail-recursive calls into flat loops, eliminating `call_function` frame allocations.
  - **Code Sinking (`CodeSinkingPass`)**: Sinks pure variable definitions into the specific branch where they are consumed.
  - **Dead Argument Elimination (`DeadArgumentEliminationPass`)**: Strips unused parameters from private/unexported internal functions across caller and callee sites.
  - **Branch Merging & Folding (`BranchMergingPass`, `BranchFoldingPass`)**: Merges adjacent `if` blocks with identical conditions and strips empty branch bodies and `Mir::Nop` instructions.
  - **Set Variable Folding (`SetVariableFoldingPass`)**: Coalesces consecutive assignments into multi-variable `variable::set_values` across block boundaries.
  - **Condition Inversion (`try_invert_action`)**: Inverts comparison operators directly when branching, eliminating redundant `then` blocks and negations.
  - **Else Flattening**: Inlines `else` blocks when preceding `then` branches terminate unconditionally via `return`, `break`, or `continue`.

### Compiler Core & Lowering
- **Short-Circuit Logical Evaluation**: Pure `Hir::And` and `Hir::Or` nodes lower directly into branching control flow, removing intermediate synthetic boolean flags (`__ht = 1.0/0.0`).
- **Class Field Lowering to `Hir::Index` / `Hir::Set`**: Class slot reads and writes lower into structured indexing primitives rather than direct `get_list_value` / `set_list_value` actions, enabling dead code elimination and slot-level optimizations.
- **Constructor Slot Inlining**: Classes without explicit `__init__` constructors populate instance slots directly via `Hir::List(slot_ids)` without intermediary zero-allocation sequences.
- **In-Place Return Target Binding**: Value-producing expressions in `return` statements are bound directly to destination parameter slots, bypassing intermediate copy variables.
- **Cached Class Slot Optimization**: Repeated reads of fields on `@cached` / `cached class` declarations within a block are automatically cached into local variables at entry.
- **Lvalue Auto-Allocation for Variable Parameters**: Passing object fields (`self.field`) or index expressions (`arr[i]`) into actions expecting `variable`-typed arguments (e.g. `variable::append_value`, `variable::random_number`) automatically generates a temporary variable and subsequent write-back (`set_list_value` / `set_map_value`).

### Standard Library (`std/`)
- **`array<T>` (`std/primitives/code/array.jc`)**:
  - `push` and `append` now invoke native `variable::append_value` directly, removing single-element array allocations (`[item]`) and `append_list` overhead.
  - Added native methods: `join(separator, prefix, postfix) -> text`, `insert(index, value) -> array<T>`, `remove(value, mode) -> array<T>`, and `remove_at(index) -> array<T>`.
- **`map<K, V>` (`std/primitives/code/map.jc`)**:
  - Added native methods: `get(key, default_value) -> V`, `sort(order, by) -> map<K, V>`, `remove(key) -> map<K, V>`, and `clear() -> map<K, V>`.
- **`iterator` (`std/primitives/code/iterator.jc`)**:
  - Replaced quadratic array copying in iterator consumers (`to_array`) with native `.push()`.
  - Replaced raw platform calls with idiomatic `.len()`, `.keys()`, `ceil()`, and `floor()`.
- **`text` (`std/primitives/code/text.jc`)**:
  - Added `chars() -> array<text>` using native `set_variable_text_to_chars`.
  - Added `ends_with(suffix: text) -> boolean` using native `if_variable_text_ends_with`.
  - Enforced concrete return typing on `split(delimiter) -> array<text>`.
- **Spatial Primitives (`location.jc`, `vector.jc`)**:
  - `location`: Replaced multi-action coordinate arithmetic with native `shift_all_coordinates`; added `shift`, `shift_coord`, `distance`, and `direction_to`.
  - `vector`: Replaced manual 4-action vector normalization with native `variable::set_vector_length(self, 1)`; added `set_length`, `angle_to`, and `midpoint`.
- **Math & Numeric Primitives (`number.jc`, `math/core.jc`, `matrix.jc`)**:
  - Added `tan()`, `clamp()`, and default precision parameter `n = 0` to `math::round()`.
  - Fixed mutation semantics for `__iremainder__` and `__ipow__` (`return self = ...`).
  - Added equality aliases to `Matrix.__equals__` and simplified `Matrix.__not_equals__`.
- **AI, Security & Effects Modules (`astar.jc`, `leaderboard.jc`, `item.jc`, `nn.jc`, `modchecker.jc`)**:
  - `astar`: Fixed obstacle collision logic bug (`or not is_walkable(end)`); migrated to standard collection methods (`.len()`, `.reverse()`, `.remove()`, `abs()`, `sqrt()`).
  - `leaderboard`: Migrated to native `remove()`, `sort()`, and `replace()`.
  - `item`: Fixed setter chaining on `item.id`; updated metadata iteration to idiomatic `for key, value in tags`.
  - `nn`: Converted internal layer arrays to native `.push()`.

### Static Analysis & Diagnostics
- **New Compiler Warnings**:
  - `W0003: UnusedVariable`: Warns on local variables that are declared and assigned but never read; respects dynamic `%` placeholders and `_` prefixes.
  - `W0004: OverwrittenAssignment`: Warns when a variable assignment is overwritten before ever being read.
  - `W0005: LoopInvariant`: Warns when an assignment inside a loop header or body depends strictly on loop-invariant values.
  - `W0006: ConsecutiveIfSameCondition`: Warns when two consecutive `if` statements evaluate identical, unmodified condition expressions.
  - `W0007: UnusedParameter`: Warns on unreferenced parameters in private/non-exported functions.
  - `W0008: DeadLoop`: Warns on loops with constant-false conditions or side-effect-free bodies.
  - `W0009: LoopInvariantBranch`: Warns on loop branches whose conditions are loop-invariant.
- **Diagnostic Precision**:
  - Added string interpolation awareness (`$var` / `${var}`) in text literals to prevent false unused variable/parameter warnings.

### Bug Fixes
- **Action Argument Invalidation in HIR Copy Propagation**: Fixed a bug where positional action arguments writing to target variables (e.g. `variable::random_number(rsy, 0.8, 1, "FALSE")`) failed to invalidate active copy mappings, erroneously folding subsequent expressions (such as `0 - (1 - rsy)`) into constants at compile time.
- **Unified Action Mutation Tracking**: Added `collect_action_written_vars` to systematically resolve mutating action arguments against platform schemas across `CopyPropagationPass`, `LoopInvariantCodeMotionPass`, `LoopUnswitchPass`, `LoopFusionPass`, and `GuardFactoringPass`.
- **Inlined Variable Resolution in Condition Peeling**: Fixed an issue where inlined function parameter bindings (e.g. `abs(dy)` in compound conditions) lost AST identity during MIR lowering. Introduced scoped symbol tracking (`named_var_subst`) with rollback across `peel_cond_lets`, `lower_condition_action`, and `lower_if`.
- **Generic Type Annotations on Uninitialized Declarations**: Fixed a compiler crash (`unknown type: T`) when encountering uninitialized generic variables (`var val: T;`) within generic class and function bodies.
- **Handler Action Splitting (`split_long_handlers`)**:
  - Fixed asymmetric dynamic argument detection: dynamic argument passing is now triggered if either side of the split boundary involves dynamic `%` variables.
  - Implemented recursive line placeholder extraction (`%var_line(...)`), capturing nested placeholders and embedded variable names across map keys, items, and blocks.
- **Temporary Variable Resolution in String Interpolation**: Ensured all variable scopes (`%var_line`, `%var_local`, `%var`, `%var_save`) are replaced during alias resolution in `CodeGen::emit_text_content`.
- **Dead Code Elimination Dynamic Target Safety**: Prevented DCE from stripping assignments to variables containing dynamic placeholder targets (`prefix_%var_line(k)`).
- **Scope Safety in Redundant Else Elimination**: Restricted zero-assignment stripping in `else` blocks strictly to compiler-generated temporaries (`__mt`) within `Mir::Line` scope, preventing erasure of prior state in user variables.
- **Copy Coalescing String Literal Safety**: Prevented merging or pruning of temporary variables referenced within string literals and excluded variable names containing `%`.
- **Schema Conformity for `set_variable_get_list_value`**: Fixed parameter name in `mir/exprs.rs` from `"index"` to `"number"` according to the JustMC schema.

### Language Server & Tooling (`jmc-analyzer`)
- **Diagnostic Integration**: Connected compiler warning codes (`W0003`–`W0009`) to the language server diagnostic stream.
- **Type & Parameter Inlay Hints**: Enhanced inlay hint rendering for inferred variable types and function call parameters.
- **Completion & Semantic Highlighting**: Updated TextMate grammar and context-aware completion providers for new decorators, stdlib collection methods, and keywords.
- **LSP Test Suite**: Added dedicated integration test coverage for diagnostic emission, hover, completion, and signature help.

### Runtime Simulation (`jmcmock`)
- **Platform Action Support**: Added mock handlers for `shift_all_coordinates`, `text_to_chars`, `text_ends_with`, and collection mutations.
- **Runtime Expression Evaluation**: Improved dynamic placeholder resolution, supporting nested `%var_line(...)` evaluation and line-scope propagation.
- **Declarative Testing**: Added `assert_log`, `clear_log`, and player resolution support for JSON-driven multi-step scenarios (`--scenario`).
