# jmcdata

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

JustMC Schema Model: definitions for actions, events, game values, selectors, arguments, and their JSON representations.

This crate serves two components:
1. The `jmcc` compiler uses it to validate `.jc` source programs and generate JSON modules.
2. The `jmcmock` emulator uses the exact same model types to load and execute emitted JSON modules, ensuring the compiler and runtime cannot diverge.

## Structure

| File / Directory | Purpose |
|---|---|
| `assets/actions.json` | 956 actions: 332 `variable`, 227 `entity`, 205 `player`, 128 `world`, 35 `select`, 12 `repeat`, 11 `code`, 6 `controller` |
| `assets/events.json` | 186 events |
| `assets/game_values.json` | 196 game values |
| `assets/selectors.json` | Selectors: 8 player, 11 entity, 9 game value |
| `assets/tests/test1.json` | Sample reference module for deserialization testing |
| `build.rs` | Code generation driver invoking generators to produce `generated.rs` |
| `build/` | Generator modules: `assets`, `enums`, `actions`, `op_builder`, `lookups`, `library`, `util` |
| `src/generated.rs` | Includes generated code from `$OUT_DIR/generated.rs` |
| `src/module.rs` | Module AST representation written by compiler and read by mock |
| `src/op_builder.rs` | Manual operation builder API |
| `src/consts.rs` | JustMC platform limits (handlers per floor, floors, total handlers) |
| `src/tests.rs` | Roundtrip and serialization unit tests |

An action schema entry defines:
- `object`: Target category (`player`, `entity`, `world`, `variable`, etc.).
- `type`: `basic`, `container`, `basic_with_conditional`, `container_with_conditional`.
- `args`: List of positional and named argument definitions.
- `assign`: Target slot for return values.
- `origin`: Argument slot populated by method call receiver (e.g., `a.greater(b)` places `a` into the `value` parameter per `origin: "value"`).
- `boolean`: Whether action functions as a conditional.
- `lambda`: Inner block executed by the action.

## Generated Code

`build.rs` executes prior to compiling the crate, parsing `assets/*.json` and writing `$OUT_DIR/generated.rs`.
**`generated.rs` is never edited manually.**

Generated definitions include:
- Enums: `ActionId`, `EventId`, `GameValueId`, `ValueType`, `ArgType`.
- Value set enums with `as_str()` (`WaitTimeUnit`, `StartProcessTargetMode`, etc.).
- `ActionArg` and `ActionDef` metadata structs.
- `Op<'_>` builder methods for each action.
- Perfect hash maps (`phf`): `get_action_id`, `get_action_def`, `get_action_def_by_id`, `get_game_value_type`, and static selector maps.

## Module Model

A module contains an array of handlers (`Line`), each representing an event, process, or function:

```
Module { handlers: Vec<Line> }
        │
        └─ Line ─┬─ line_type: Event | Process | Function
                 ├─ position: u16       — Editor line index
                 ├─ operations: Vec<Op> — Action body
                 └─ line_value ─── Event { event }
                                 └─ Fn { name, values }
```

An operation (`Op`) represents an individual action:

```
Op ─┬─ action: ActionId
    ├─ values: LiteMap<arg_name, Value>
    ├─ operations: Option<Vec<Op>>             — Container body
    ├─ conditional: Option<Conditional>        — Conditional configuration
    ├─ selection: Option<Selection>            — Target selector
    └─ is_inverted: Option<bool>               — Inverted condition
```

### Values (`Value`)

| Variant | Description | JSON Type Tag |
|---|---|---|
| `Array` | List of values | `array` |
| `Block` | Minecraft block representation | `block` |
| `Enum` | Enum variant | `enum` |
| `Item` | Inventory item | `item` |
| `Location` | Coordinate (`x, y, z, yaw, pitch`) | `location` |
| `Map` | Dictionary | `map` |
| `Number` | Numeric constant or expression | `number` |
| `Particle`, `Potion`, `Sound` | Visual and audio effects | matching names |
| `Text` | String with parsing mode (`plain`, `legacy`, `minimessage`, `json`) | `text` |
| `Variable` | Variable name and scope (`line`, `local`, `game`/`unsaved`, `save`/`saved`) | `variable` |
| `Vector` | 3D vector (`x, y, z`) | `vector` |
| `GameValue` | Platform dynamic value and selector | `gamevalue` |

## Platform Limits

```rust
MAX_HANDLERS_PER_FLOOR = 23;
MAX_FLOORS             = 15;
MAX_HANDLERS           = 345;
```

## Modifying Schema Actions

1. Edit or add the action in `jmcdata/assets/actions.json`.
2. Recompile: `cargo build -p jmcdata`.
3. If new actions were added, verify the generated builder `Op::<object>_<name>` and implement runtime emulation in `jmcmock`.

## Tests

```bash
cargo test -p jmcdata
```
