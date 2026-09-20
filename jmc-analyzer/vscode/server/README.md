# Future language server

> [!NOTE]
> The Russian version of this document is available at [README_RU.md](README_RU.md).

The VS Code client starts `jmc-analyzer lsp` over stdio when
`jmc.analyzer.enableLsp` is true. Implementation lives in
`jmc-analyzer/src/lsp/` (feature `lsp`). This directory is reserved for
any extra server assets the client may need later; it is not shipped in the
`.vsix` yet.
