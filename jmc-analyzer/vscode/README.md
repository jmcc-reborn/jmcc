# JMC Analyzer

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

VS Code extension and Language Server (LSP) for the JustCode (`.jc`) programming language from the [JMCC](https://github.com/jmcc-reborn/jmcc) compiler.

Provides full parity for both English (Modern) and Russian (Alternate) JustCode syntax (`while`/`for`, lambdas, `match`, `try`/`catch`, classes, interfaces, `export`, loop labels, Russian keywords and type annotations).

## Features

### 1. Syntax Highlighting
- Precise TextMate grammar for keywords, types, JustMC actions, operators, and string interpolation (`$var` and `${expr}`).
- Full support for Russian syntax (`функция`, `класс`, `пусть`, `перем`, `если`, `выбор`, `не`, `и`, `или`, `в`, `как`, etc.).
- Semantic Tokens via LSP in UTF-16 code units with accurate Cyrillic offset handling.

### 2. Language Server (LSP)
The built-in Language Server provides rich IDE capabilities:
- **On-the-fly Diagnostics:** Syntax, type, and semantic validation as you type with warnings (e.g., deprecated `elif`) and localized messages (RU/EN).
- **Type Inference and Inlay Hints:** Displays inferred types for unannotated variables (`: text`, `: number` for English code; `: текст`, `: число` for Russian code) and argument names for actions, functions, and constructors.
- **Hover Information:** Signatures, inferred types, variable scope annotations, aliases, and bilingual doc comments (`/// RU:` / `/// EN:`).
- **Go to Definition:** Navigation for classes, interfaces, enums, enum variants, functions, methods, and variables.
- **References & Document Highlight:** Finds references and highlights symbol usages across documents.
- **Rename:** Safe symbol renaming across the workspace.
- **Document & Workspace Symbols:** Outlines and search with class/method hierarchy.
- **Completion (IntelliSense):** Actions, properties, methods, and global symbols.
- **Signature Help:** Parameter prompts when calling actions and methods.
- **Document Formatting:** Built-in code formatting via the compiler's formatter.

## Building and Installation

Package the extension with `jmc-analyzer`:

```shell
cargo run -p jmc-analyzer -- pack
```

The resulting `.vsix` file is placed in `jmc-analyzer/out/justcode-lang-<version>.vsix`. Install in VS Code:

```shell
code --install-extension jmc-analyzer/out/justcode-lang-0.1.0.vsix
```
