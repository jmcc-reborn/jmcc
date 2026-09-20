# jmc-analyzer

> [!NOTE]
> Английская версия доступна в [README.md](README.md).

Данные языка JustCode, сборка VS Code-расширения и заготовка language server.

| Путь | Зачем |
|---|---|
| `data/syntax.json` | машинное описание синтаксиса: ключевые слова, объекты, операторы, как запускать LSP |
| `data/justcode.tmLanguage.json` | TextMate-грамматика под текущий лексер `jmcc` |
| `data/language-configuration.json` | скобки, комментарии, отступы для редактора |
| `vscode/` | клиент VS Code; грамматика копируется сюда из `data/` в `build.rs` |
| `src/lsp/` | полнофункциональный LSP сервер (диагностика, автодополнение, hover, inlay hints, goto def, rename и др.) |
| `build.rs` | копирует `data/` в `vscode/` и пакует `.vsix` |

Раньше подсветкой занималось стороннее `jmcc-helper`. Оно не знает циклы
`while`/`for`, лямбды, `match`, `try`/`catch`, интерфейсы, `export`, метки
циклов и добрую половину ключевых слов текущего издания. Грамматика здесь
поддерживает как английский (Modern), так и русский (Alternate) синтаксис,
сверяясь с `jmcc/src/ast/lexer/` и справочником в корневом `README.md`.

## CLI

```shell
cargo run -p jmc-analyzer -- syntax     # каталог синтаксиса
cargo run -p jmc-analyzer -- pack       # сборка .vsix расширения
cargo run -p jmc-analyzer -- lsp        # запуск Language Server (stdio)
```

`cargo build -p jmc-analyzer` пишет
`jmc-analyzer/out/jmc-analyzer-<версия>.vsix`.
