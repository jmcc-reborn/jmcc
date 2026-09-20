# jmcmock

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

JustMC Mock Runtime: loads compiled JSON emitted by `jmcc` and simulates execution offline.

The mock runtime tests `.jc` programs without requiring a running Minecraft server. It uses the exact data model from [`jmcdata::module`](../jmcdata/README.md) and validates runtime behavior (events, threads, memory, execution order, errors).

Static checks (types, names, argument signatures) are performed by the compiler. The mock runtime catches dynamic runtime conditions.

## Running

```bash
cargo build -p jmcmock

# Compile module first, then simulate
RUST_LOG=warn cargo run -p jmcc -- compile jmcc/tests/nn.jc --emit json
./target/debug/jmcmock jmcc/tests/nn.json -e player_chat --chat '@train_nn'

# List module contents
./target/debug/jmcmock jmcc/tests/nn.json --list
```

### CLI Arguments

| Option | Description |
|---|---|
| `<module.json>` | Compiled module emitted by `jmcc --emit json` |
| `-e, --event <NAME>` | Event to trigger (repeatable, executes in specified order; defaults to `world_start`) |
| `--chat <TEXT>` | Message payload for `event_chat_message` (`%event_chat_message%`) |
| `--slot <N>` | Slot index for `event_slot` |
| `-p, --player <NAME>` | Add player to mock world (repeatable) |
| `--no-default-player` | Do not create default `Dev` player |
| `--unimplemented <error\|record\|ignore>` | Handling of unimplemented actions (default: `error`) |
| `--step-limit <N>` | Maximum operations per run (default: 5,000,000) |
| `--ticks <N>` | Virtual tick budget per event (default: 100) |
| `--save <FILE>` | Persistent `save` scope file (loaded at startup, flushed on exit) |
| `--list` | Display events, functions, and processes, then exit |

World event logs are output to `stdout`; diagnostics and errors to `stderr`.

## Supported Actions

The mock implements computational and control flow operations:

| Schema Object | Implemented in Mock | In Schema | Scope |
|---|---:|---:|---|
| `variable` | 49 | 332 | Calculations, lists, maps, text, value conditionals |
| `code` | 10 | 11 | Wait, return, calls, `measure_time`, exceptions |
| `controller` | 3 | 6 | `measure_time`, `exception`, `do_not_run` |
| `repeat` | 7 | 12 | Loops (`repeat_while`, `repeat_forever`, etc.) |
| `select` | 32 | 35 | Target selection |
| `player` | 8 | 205 | Messages, sounds, boss bars, event damage |
| `world` | 2 | 128 | Event damage, event cancellation |
| `entity` | 0 | 227 | Minecraft client/server-specific world actions |
| **Total** | **111** | **956** | |

Unimplemented actions fail by default with `RuntimeError::Unimplemented`.

## World and Event Log

The mock world contains players and entities. By default, it initializes with a single player (`Dev`).

Action effects log per target frame:
```
[tick] <player|entity> <name>: <action> <details>
```

```
[0] player Dev: player_send_message Training started...
[0] player Dev: player_send_message [[], 0.01]
```

## Variable Scopes and Lifetimes

| Scope | Lifetime | Storage |
|---|---|---|
| `line` | Single invocation frame | Frame storage |
| `local` | Call stack of event or process | Process storage |
| `game` | Runtime lifetime | Runtime memory |
| `save` | Persistent across runs | Runtime memory + file |

## Runtime Validation

| Error | Condition |
|---|---|
| `UndefinedVariable` | Variable does not exist in scope when read |
| `UnsetVariable` | Variable declared but uninitialized |
| `UnknownFunction`, `UnknownProcess`, `UnknownEvent` | Handler missing from module |
| `MissingParameter`, `UnknownParameter` | Call arguments mismatch parameters |
| `MissingArgument`, `UnknownArgument` | Action schema argument discrepancy |
| `Unimplemented` | Action not implemented by mock |
| `DivisionByZero`, `IndexOutOfRange` | Arithmetic and list indexing errors |
| `StepLimitExceeded` | Exceeded `--step-limit` |
| `Raised` | Unhandled `throw` or exception |

## Testing

```bash
cargo test -p jmcmock
```

`jmcmock` includes unit and runtime simulation tests in `tests/runtime_tests.rs`.
