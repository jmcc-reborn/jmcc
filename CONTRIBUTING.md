# Contributing to JMCC

Thank you for your interest in contributing to the JustMC Code Compiler!

## Getting Started

### Prerequisites
- **Rust Toolchain**: `nightly` channel (specified in [`rust-toolchain.toml`](rust-toolchain.toml)).
- **Components**: `rustfmt`, `clippy`.

```bash
rustup toolchain install nightly
rustup component add rustfmt clippy --toolchain nightly
```

### Building the Project
```bash
cargo build
```

To build optimized binaries:
```bash
cargo build --release
```

## Development Workflow

### 1. Code Formatting
Before submitting changes, ensure your code is formatted according to the project style:
```bash
cargo fmt --all -- --check
```
To automatically apply formatting:
```bash
cargo fmt --all
```

### 2. Linting
The workspace enforces strict clippy lints:
```bash
cargo clippy --workspace -- -D warnings
```

### 3. Testing
Run the complete test suite across all workspace crates (`jmcc`, `jmcdata`, `jmcmock`, `jmc-analyzer`):
```bash
cargo test --workspace
```

### 4. Language Specification & Standard Library
- The normative language specification is in [`SPECIFICATION.md`](SPECIFICATION.md) ([Russian](SPECIFICATION_RU.md)).
- Standard library primitives are located in [`jmcc/std/`](jmcc/std/).
- Operators (`+`, `==`, `in`, `+=`, etc.) resolve to dunder methods (`__add__`, `__equals__`, `__contains__`, `__iadd__`, etc.) on `@lang_item` classes in [`jmcc/std/primitives/code/`](jmcc/std/primitives/code/).

## Submitting a Pull Request
1. Fork the repository and create your feature branch from `main`.
2. Commit your changes with descriptive commit messages.
3. Verify that `cargo fmt`, `cargo clippy`, and `cargo test` pass cleanly.
4. Push your branch to your fork and submit a Pull Request targeting `main`.
