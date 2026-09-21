# JMCC (JustMC Code Compiler)

[![CI](https://github.com/jmcc-reborn/jmcc/actions/workflows/ci.yml/badge.svg)](https://github.com/jmcc-reborn/jmcc/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-Community-5865F2?logo=discord&logoColor=white)](https://discord.gg/Adx3H3zAFU)

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

JMCC is a modern optimizing compiler for the JustCode (`.jc`) programming language, targeting executable JSON modules for the JustMC platform (DiamondFire architecture on Minecraft servers).

The repository includes the compiler, code generator, formal JustMC action schema, execution emulator (mock runtime), and Language Server Protocol (LSP) extension.

The complete language specification is available in [SPECIFICATION.md](SPECIFICATION.md) ([RU](SPECIFICATION_RU.md)), and the practical developer guide is in [GUIDE.md](GUIDE.md) ([RU](GUIDE_RU.md)).

---

## 1. Repository Architecture

The repository is structured as a unified Cargo workspace composed of four components:

| Crate / Directory | Description | Documentation |
|---|---|---|
| [`jmcc`](jmcc) | Compiler for `.jc`, multi-stage optimization pipeline (HIR, MIR), JSON code generator, and standard library ([`std/`](jmcc/std)) | [jmcc/README.md](jmcc/README.md) ([RU](jmcc/README_RU.md)) |
| [`jmcdata`](jmcdata) | Generated model of the JustMC platform (actions, events, game values, selectors) and output JSON module schema | [jmcdata/README.md](jmcdata/README.md) ([RU](jmcdata/README_RU.md)) |
| [`jmcmock`](jmcmock) | Virtual machine and mock runtime: executes compiled JSON modules, emulates events, threads, scheduler, and memory | [jmcmock/README.md](jmcmock/README.md) ([RU](jmcmock/README_RU.md)) |
| [`jmc-analyzer`](jmc-analyzer) | Language Server (LSP), TextMate grammar, and editor extension (Visual Studio Code) | [jmc-analyzer/README.md](jmc-analyzer/README.md) ([RU](jmc-analyzer/README_RU.md)) |

The compiler (`jmcc`) and the mock runtime (`jmcmock`) are synchronized via the shared schema crate `jmcdata`. Actions and argument structures are modified centrally in `jmcdata/assets/*.json`.

---

## 2. Requirements & Building

### 2.1. Prerequisites
- Rust compiler: `nightly` channel (pinned in [`rust-toolchain.toml`](rust-toolchain.toml)).
- Rust Edition: `2024`.

### 2.2. Building from Source
Build all workspace components in optimized release mode:

```bash
cargo build --release
```

Compiled binaries are placed in `target/release/`:
- `target/release/jmcc` — JustCode compiler and formatter.
- `target/release/jmcmock` — Program simulation and debugging runtime.

---

## 3. Command Line Interface (CLI)

The `jmcc` CLI tool provides commands for compilation and code formatting.

### 3.1. `compile` Command
Compiles a `.jc` source file into a JustMC JSON module:

```bash
jmcc compile [path_to_file_or_project] [options]
```

If no file path is specified, the compiler automatically discovers `jmcc.toml` in the current or parent directories and builds the entire project.

Parameters for `compile`:

| Flag | Default | Description |
|---|---|---|
| `-o, --output <PATH>` | `<filename>.json` | Destination path for output JSON |
| `-O, --opt-level <0..3>` | `2` | Optimization level (0 = disabled, 1..3 = aggressive passes) |
| `--profile <NAME>` | `dev` | Build profile from `jmcc.toml` (`dev`, `release`, or custom) |
| `--release` | `false` | Shortcut for `--profile release` (`-O 3`, `--emit json`) |
| `--target <TARGET>` | `justmc` | Target platform |
| `--edition <2023\|2026>` | `2026` | Language edition (2026 recommended) |
| `--passes <A,B,...>` | all active for `-O` | Comma-separated explicitly enabled optimization passes |
| `--disable-passes <A,B,...>` | none | Comma-separated disabled optimization passes |
| `--emit <TYPES>` | `ast,hir,mir,json` | Comma-separated list of generated artifacts |
| `-u, --upload [TARGET]` | `official` (when `-u` passed) | Upload compiled module to server (`official` API or `webhook`) and output Minecraft `/module loadUrl` command |
| `--upload-target <TARGET>` | from `jmcc.toml` or `official` | Target service for uploading: `official` or `webhook` |
| `--webhook-url <URL>` | from `jmcc.toml` or default | Custom Discord webhook URL for unofficial upload target |
| `--locale, --lang <LANG>` | system default | Diagnostic message language (`ru`, `en`) |

Example compilation invocations:
```bash
# Build the project in the current directory with release profile
jmcc compile --release

# Compile a standalone script and upload directly using official JustMC API
jmcc compile main.jc -O 3 -u

# Compile and upload via Discord webhook using a custom webhook URL
jmcc compile -u webhook --webhook-url "https://discord.com/api/webhooks/..."
```

### 3.2. `new` Command
Initializes a new JustCode package (similar to `cargo new`):

```bash
jmcc new <path> [options]
```

Parameters for `new`:

| Flag | Default | Description |
|---|---|---|
| `--bin` | default | Create an application package with `src/main.jc` |
| `--lib` | false | Create a library package with `src/lib.jc` |
| `--edition <2023\|2026>` | `2026` | Language edition to set in `jmcc.toml` |
| `--name <NAME>` | directory name | Set package name explicitly |
| `--vcs <VCS>` | `git` | Initialize repository (`git`, `none`) |

### 3.3. `format` Command
Automated source code formatter for JustCode:

```bash
jmcc format <file_or_directory> [--check]
```

- If a directory is specified, formatting is performed recursively for all `.jc` files.
- The `--check` flag verifies formatting style without modifying files, returning a non-zero exit code on discrepancies.

### 3.4. Project System (`jmcc.toml` Manifest)

Any directory containing a `jmcc.toml` file is treated as a JustCode project. The compiler automatically discovers the entry point (`src/main.jc`, `main.jc`, `src/lib.jc`, or `lib.jc`), resolves dependencies, and applies build profiles.

Example `jmcc.toml` manifest:

```toml
[project] # or [package]
name = "my_quest_game"
version = "0.1.0"
edition = 2026
# entry = "src/main.jc" # auto-detected by default

[dependencies]
# Local path dependency
common_utils = { path = "../common_utils" }

# Git dependency
network = { git = "https://github.com/example/network.git", branch = "main" }

[profile.dev]
opt_level = 1

[profile.release]
opt_level = 3
emit = ["json"]

# Optional module upload configuration:
[upload]
target = "official" # or "webhook"
# webhook_url = "https://discord.com/api/webhooks/..." # custom Discord webhook URL
# enabled = true # auto-upload on compile

# Workspace support:
# [workspace]
# members = ["crates/*"]
```

In source code, dependencies are imported by package name:
```jc
import "common_utils";
import "network/client.jc";
```

---

## 4. Quick Start

### 4.1. Sample Program (Bilingual Syntax)
JustCode supports English and Russian keywords with full parity.

#### English syntax:
```jc
event<player_join> {
    var name = value::name<current>;
    player::message("Welcome to the server, ${name}!");

    var items = ["stone", "wood", "iron"];
    for item in items {
        player::message("Item: ${item}");
    }
}
```

#### Equivalent Russian syntax:
```jc
событие<player_join> {
    перем имя = value::name<current>;
    player::message("Добро пожаловать на сервер, ${имя}!");

    перем список = ["камень", "дерево", "железо"];
    для предмет в список {
        player::message("Предмет: ${предмет}");
    }
}
```

### 4.2. Compilation and Testing
1. Compile to JSON:
```bash
jmcc compile game.jc -o game.json
```
2. Run in mock runtime to verify logic:
```bash
jmcmock run game.json --event player_join
```

---

## 5. Language Features Overview

- **Bilingual Lexical Syntax**: All keywords have full Russian and English equivalents (`function`/`функция`, `var`/`перем`, `if`/`если`, `while`/`пока`, `match`/`выбор`).
- **Static Typing and Type Inference**: Supports primitives (`number`, `text`, `boolean`), collections (`array<T>`, `map<K, V>`), platform types (`location`, `item`, `vector`), and generics.
- **Standard Library Operator Model**: Operators (`+`, `*`, `==`, `in`, `+=`, etc.) are not hardcoded into the compiler, but resolve to `@lang_item` dunder methods (`__add__`, `__equals__`).
- **Four Variable Storage Scopes**:
  - `line` — Local to handler line (default in Edition 2026).
  - `local` — Local to thread call stack.
  - `game` — Global game world state.
  - `save` — Persistent across server restarts.
- **Object-Oriented Programming**: Classes (`class`), interfaces (`interface`) with multiple inheritance (`extends`), constructors (`__init__`), and inline classes.
- **Functional Features**: Lambda expressions (`(x) => x + 1`), function types (`Fn<T, R>`), inline functions (`inline function`), overloads (`@overload`).
- **Pattern Matching**: The `match` construct supports multiple alternatives (`1 | 2`), guards (`if guard`), and wildcards (`_`).
- **Error Handling**: Structured exception handling via `try` / `catch` / `throw`.
- **JustMC Platform Integration**: Action calls `category::action<selector>(arguments)`, dynamic values `value::name`, event handlers `event<name>`.

Detailed grammar and syntax rules are defined in [SPECIFICATION.md](SPECIFICATION.md) ([RU](SPECIFICATION_RU.md)), and usage guides are in [GUIDE.md](GUIDE.md) ([RU](GUIDE_RU.md)).

---

## 6. Testing and Verification

The repository includes a comprehensive automated test suite (over 110 tests across all crates):

```bash
# Type check and build verification
cargo check

# Linter rules and code style
cargo clippy -- -D warnings

# Execute entire test suite
cargo test
```

The test harness covers:
1. Lexer, bilingual syntax, and dunder operator mapping unit tests.
2. Compiler integration tests (`tests/compile_tests.rs`): complex scenarios and real-world scripts.
3. Formatter tests (`tests/format_tests.rs`): idempotent roundtripping on all `std/` modules.
4. Runtime emulator tests (`jmcmock/tests/runtime_tests.rs`): byte-level JSON execution verification for variables, loops, events, and timers.
5. Crash reporting (`src/panic_handler.rs`), package management, and lockfile tests (`src/project/`).

---

## 7. Additional Documentation

- [SPECIFICATION.md](SPECIFICATION.md) ([RU](SPECIFICATION_RU.md)) — Normative technical specification for JustCode, grammar, and compiler.
- [GUIDE.md](GUIDE.md) ([RU](GUIDE_RU.md)) — Practical developer guide and cookbook.
- [jmcc/README.md](jmcc/README.md) ([RU](jmcc/README_RU.md)) — Compiler architecture, HIR/MIR IRs, optimization pipeline, and code generation.
- [jmcdata/README.md](jmcdata/README.md) ([RU](jmcdata/README_RU.md)) — JustMC schema, asset generation.
- [jmcmock/README.md](jmcmock/README.md) ([RU](jmcmock/README_RU.md)) — Mock runtime guide and simulation environment.
- [jmc-analyzer/README.md](jmc-analyzer/README.md) ([RU](jmc-analyzer/README_RU.md)) — Language Server and VS Code extension.

---

## 8. Community

- **Discord**: Join our community for questions, discussions, and updates: [https://discord.gg/Adx3H3zAFU](https://discord.gg/Adx3H3zAFU)

