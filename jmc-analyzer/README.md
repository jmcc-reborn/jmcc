# jmc-analyzer

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

Language data for JustCode, VS Code extension packaging, and Language Server (LSP) implementation.

| Path | Purpose |
|---|---|
| `data/syntax.json` | Machine-readable syntax specification: keywords, objects, operators, LSP launch configuration |
| `data/justcode.tmLanguage.json` | TextMate grammar targeting the current `jmcc` lexer |
| `data/language-configuration.json` | Brackets, comments, and indentation rules for code editors |
| `vscode/` | VS Code client; grammar is copied here from `data/` via `build.rs` |
| `src/lsp/` | Feature-complete Language Server (diagnostics, autocomplete, hover, inlay hints, goto definition, rename, formatting) |
| `build.rs` | Copies `data/` into `vscode/` and packages `.vsix` extension |

The grammar and language server support both English (Modern) and Russian (Alternate) syntax, verified against `jmcc/src/ast/lexer/` and the root `README.md`.

## CLI

```shell
cargo run -p jmc-analyzer -- syntax     # Syntax catalog
cargo run -p jmc-analyzer -- pack       # Package .vsix extension
cargo run -p jmc-analyzer -- lsp        # Launch Language Server (stdio)
```

`cargo build -p jmc-analyzer` produces `jmc-analyzer/out/justcode-lang-<version>.vsix`.
