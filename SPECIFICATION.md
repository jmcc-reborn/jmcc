# JustCode (.jc) Language and JMCC Compiler Specification

> [!NOTE]
> The Russian version of this specification is available at [SPECIFICATION_RU.md](SPECIFICATION_RU.md).

This document serves as the normative technical specification for the JustCode (`.jc`) programming language, the architecture of the JMCC ("JustMC Code Compiler") compiler, the standard library (`std/`), and the executable program format for the JustMC target platform (a DiamondFire-derivative architecture within the Minecraft environment).

---

## 1. Normative Provisions and Scope

### 1.1. Document Status
This document defines the lexical grammar, formal syntax, operator precedence and associativity, static type system, execution semantics, closure transformation (Lambda Lifting), memory model, dunder method overloading protocols, standard library, platform limits, output JSON module specification, command-line interface, and diagnostic error code catalog.

### 1.2. Terminology
Requirement keywords are interpreted in accordance with RFC 2119:
- **MUST** / **REQUIRED**: Absolute requirement for implementations or code.
- **MUST NOT**: Absolute prohibition.
- **SHOULD** / **RECOMMENDED**: Valid reasons may exist in particular circumstances to ignore, but full implications must be understood.
- **MAY**: Optional behavior.

### 1.3. Language Editions
The compiler implements two normative language editions:
1. **Edition 2026 (`edition = 2026`)**: The normative default mode.
   - Strict module encapsulation: Only symbols explicitly marked with the `export` keyword are accessible to external modules.
   - Name mangling of imported symbols to prevent collisions in the platform's global address space.
   - Line scope (`line`) by default for all unannotated variables.
   - Strict type inference: Unresolved expression types produce compilation error `E0057`.
2. **Edition 2023 (`edition = 2023`)**: Backward-compatibility mode.
   - Direct AST merging of imported files without namespace isolation.
   - Local thread scope (`local`) by default.
   - Unresolved expression types fall back to the dynamic `unknown` type.

---

## 2. Lexical Structure

### 2.1. Encoding and Case Sensitivity
1. Source files (`.jc`) MUST be encoded in UTF-8 without a Byte Order Mark (BOM).
2. The language is strictly case-sensitive across all identifiers, keywords, and literals.

### 2.2. Whitespace and Line Continuations
1. Whitespace characters include space (`U+0020`), horizontal tab (`U+0009`), carriage return (`U+000D`), and newline (`U+000A`).
2. Physical line continuations are supported via a trailing backslash `\` followed by optional whitespace and a newline: `\\[ \t]*\r?\n`.

### 2.3. Comments
1. **Single-line comments**: Begin with `//` and extend to the newline character `\n` or the end of the file.
2. **Multi-line comments**: Begin with `/*` and terminate with `*/`. Arbitrary recursive nesting of multi-line comments is supported: `/* outer /* nested */ outer */`.

### 2.4. Identifiers
The following lexical forms of identifiers are defined:

1. **Standard Identifier**: A sequence starting with a Latin or Cyrillic letter or an underscore `_`, followed by letters, digits, or `_`:
   ```regex
   [a-zA-Z_а-яА-ЯёЁ][a-zA-Z0-9_а-яА-ЯёЁ]*
   ```
2. **Escaped Identifier (Backtick Identifier)**: An arbitrary sequence of characters (including whitespace, mathematical symbols, and Minecraft placeholders) enclosed in backticks:
   ```regex
   `[^`\r\n]+`
   ```
3. **Scoped Identifier**: A memory scope prefix immediately preceding a backtick:
   - Line scope: `` l`name` `` or `` line`name` ``.
   - Local thread scope: `` local`name` ``.
   - Global game scope: `` g`name` `` or `` game`name` ``.
   - Persistent save scope: `` s`name` `` or `` save`name` ``.
   - Compile-time constant: `` i`name` `` or `` inline`name` ``.
   - JMCC system scope: `` j`name` `` or `` jmcc`name` ``.
4. **Placeholder Syntax**: `%identifier%` with an optional suffix (e.g., `%player%_gold`), lowering to a JustMC platform macro substitution.

### 2.5. Bilingual Keywords
The lexer maps equivalent English (Modern) and Russian (Alternate) keywords into identical AST tokens:

| AST Token | English Keyword | Russian Equivalents | Semantic Description |
|---|---|---|---|
| `Token::Import` | `import` | `импорт` | Import of modules and dependencies |
| `Token::Export` | `export` | `экспорт` | Export of symbols from module (Edition 2026) |
| `Token::From` | `from` | `из` | Source module in `from "..." import` |
| `Token::Var` | `var` | `переменная`, `перем`, `пусть` | Mutable variable declaration |
| `Token::Const` | `const` | `константа` | Constant declaration |
| `Token::Function` | `function` | `функция` | Function declaration |
| `Token::Def` | `def` | `определение` | Alternative function declaration |
| `Token::Fun` | `fun` | `действие` | JustMC platform action invocation |
| `Token::Process` | `process` | `процесс` | Asynchronous coroutine process declaration |
| `Token::Event` | `event` | `событие` | Platform event handler |
| `Token::Class` | `class` | `класс` | Class declaration |
| `Token::Interface` | `interface` | `интерфейс` | Interface declaration |
| `Token::Implements`| `implements` | `реализует` | Class interface implementation |
| `Token::Extends` | `extends` | `расширяет` | Class and interface inheritance |
| `Token::Enum` | `enum` | `перечисление` | Strongly-typed enumeration declaration |
| `Token::TypeAlias` | `typealias`, `type` | `тип`, `псевдоним` | Type alias declaration |
| `Token::Inline` | `inline` | `встраиваемый` | Inlining modifier |
| `Token::Line` | `line` | `строка` | Line scope modifier |
| `Token::Local` | `local` | `локальный` | Local thread call-stack scope modifier |
| `Token::Game` | `game` | `игра` | Global game scope modifier |
| `Token::Save` | `save` | `сохранить`, `сохранение` | Persistent database scope modifier |
| `Token::Ref` | `ref` | `ссылка` | Pass-by-reference parameter modifier |
| `Token::If` | `if` | `если` | Conditional statement |
| `Token::Else` | `else` | `иначе` | Alternative conditional branch |
| `Token::Elif` | `elif` | `иначе_если`, `иначеесли` | Intermediate conditional branch (deprecated -> `match`) |
| `Token::While` | `while` | `пока` | Pre-condition loop |
| `Token::For` | `for` | `для` | Iteration loop |
| `Token::In` | `in` | `в` | Membership test and loop operator |
| `Token::Break` | `break` | `прервать` | Loop break statement |
| `Token::Continue` | `continue` | `продолжить` | Next loop iteration statement |
| `Token::Return` | `return` | `вернуть`, `возврат` | Return from function |
| `Token::Match` | `match` | `выбор`, `сопоставить` | Pattern matching statement |
| `Token::Case` | `case` | `вариант`, `случай` | Pattern matching arm |
| `Token::Default` | `default` | `по_умолчанию` | Default matching arm |
| `Token::Try` | `try` | `попытка` | Exception handling block |
| `Token::Catch` | `catch` | `исключение`, `перехват`, `поймать` | Exception handler |
| `Token::Throw` | `throw` | `выбросить`, `бросить` | Exception throwing statement |
| `Token::As` | `as` | `как` | Static type cast |
| `Token::Not` | `not` | `не` | Logical negation |
| `Token::And` | `and` | `и` | Logical conjunction |
| `Token::Or` | `or` | `или` | Logical disjunction |
| `Token::True` | `true` | `истина`, `правда` | Boolean true literal |
| `Token::False` | `false` | `ложь` | Boolean false literal |
| `Token::Plain` | `plain` | `простой` | Plain text prefix (`p"..."`) |
| `Token::Legacy` | `legacy` | `устаревший` | Minecraft text prefix (`l"..."`) |
| `Token::Minimessage`| `minimessage` | `минисообщение` | MiniMessage text prefix (`m"..."`) |
| `Token::Json` | `json` | `джсон` | JSON component prefix (`j"..."`) |

### 2.6. Literals

#### 2.6.1. Numeric Literals
Numbers are represented as 64-bit double-precision IEEE-754 floating-point values (`f64`). Integer, fractional, and exponential notations are supported, along with digit separation using underscores `_`:
```jc
var a = 42;
var b = 3.141_592_653;
var c = 1_000_000;
var d = 1.25e-4;
```

#### 2.6.2. Boolean Literals
Boolean constants: `true` / `истина` / `правда` and `false` / `ложь`.

#### 2.6.3. Text Literals and Parsing Prefixes
String literals are enclosed in double quotes `"..."` or single quotes `'...'`. The string component parsing mode on the JustMC platform is designated by a prefix:
- `l"..."` (Legacy, default): Minecraft color codes `&a`, `&l`, `§c`.
- `m"..."` (MiniMessage): Tagged formatting (`<gold>`, `<gradient:#ff0000:#0000ff>text</gradient>`).
- `p"..."` (Plain): Raw text without tag parsing.
- `j"..."` (JSON): Serialized Minecraft Text Component.

Supported escape sequences:
- `\n` — Line feed (`U+000A`).
- `\r` — Carriage return (`U+000D`).
- `\t` — Horizontal tab (`U+0009`).
- `\"` — Double quote.
- `\'` — Single quote.
- `\\` — Backslash.
- `\$` — Escaped dollar sign (prevents string interpolation).

#### 2.6.4. String Interpolation
Inside string literals of any type, two interpolation syntaxes are supported:
1. Short: `$identifier` (expands to the variable's value).
2. Full: `${expression}` (evaluates an arbitrary expression and converts to string via `__str__`).

#### 2.6.5. Collection and Structure Literals
- **Arrays (Array)**: `[expr1, expr2, ...]`.
- **Maps (Map)**: `{expr_key1: expr_val1, expr_key2: expr_val2, ...}`.
- **NBT Structures (Minecraft SNBT)**: Prefixes `m{...}`, `n{...}`, `nbt{...}`, `minecraft_nbt{...}`:
  ```jc
  var sword = item("diamond_sword", nbt = m{
      "minecraft:damage": 0,
      "minecraft:unbreakable": {}
  });
  ```
- **Ranges**:
  - Half-open range: `start .. end` (`Range` type, excluding `end`).
  - Closed range: `start ..= end` (`RangeInclusive` type, including `end`).

---

## 3. Formal Grammar and Operator Precedence

### 3.1. Operator Precedence and Associativity Table

The table below lists operators ordered from highest precedence to lowest:

| Level | Category | Operators | Associativity | Binding Power (L/R) |
|---|---|---|---|---|
| **14** | Postfix | `.prop`, `::action`, `[subscript]`, `(call)`, `++`, `--` | Left-to-right | 26 / 27 (Inc/Dec: 28/29) |
| **13** | Type Cast | `as`, `как` | Left-to-right | 23 / 24 |
| **12** | Prefix Unary | `-`, `not`, `не`, `!`, `++`, `--` | Right-to-left | 21 (prefix) |
| **11** | Exponentiation | `^`, `**` | **Right-to-left** | 21 / 20 |
| **10** | Multiplicative | `*`, `/`, `%` | Left-to-right | 13 / 14 |
| **9** | Additive | `+`, `-` | Left-to-right | 11 / 12 |
| **8** | Ranges | `..`, `..=` | Left-to-right | 9 / 10 |
| **7** | Bitwise Shifts | `<<`, `>>` | Left-to-right | 19 / 20 |
| **6** | Bitwise Logic | `&`, `\|`, `^` | Left-to-right | 15/16, 17/18 |
| **5** | Comparisons & Membership | `<`, `<=`, `>`, `>=`, `in`, `в` | Left-to-right | 7 / 8 |
| **4** | Equality | `==`, `!=` | Left-to-right | 5 / 6 |
| **3** | Logical AND | `and`, `&&`, `и` | Left-to-right | 3 / 4 |
| **2** | Logical OR | `or`, `\|\|`, `или` | Left-to-right | 1 / 2 |
| **1** | Ternary | `? :`, `expr if cond else expr` | Right-to-left | 1 |
| **0** | Assignment | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `^=` | Right-to-left | 0 |

### 3.2. Platform Factory Pseudo-Constructor Desugaring
Unqualified function-call forms of built-in identifiers are desugared by the parser into `Expr::Constructor` AST nodes:
- `sound(...)` / `звук(...)` -> Constructor of `sound`.
- `particle(...)` / `частица(...)` -> Constructor of `particle`.
- `potion(...)` / `зелье(...)` -> Constructor of `potion`.
- `item(...)` / `предмет(...)` -> Constructor of `item`.
- `block(...)` / `блок(...)` -> Constructor of `block`.
- `value(...)` / `значение(...)` -> JustMC dynamic value read.
- `enum(...)` / `перечисление(...)` -> Enum variant instantiation.

### 3.3. Formal Grammar (EBNF)

```ebnf
Program ::= Statement* EOF ;

Statement ::= ImportStmt
            | VarDeclStmt
            | ConstDeclStmt
            | FunctionDecl
            | ProcessDecl
            | EventDecl
            | ClassDecl
            | InterfaceDecl
            | EnumDecl
            | TypeAliasDecl
            | IfStmt
            | WhileStmt
            | ForStmt
            | MatchStmt
            | TryCatchStmt
            | ReturnStmt
            | BreakStmt
            | ContinueStmt
            | AssignStmt
            | ExprStmt ;

ImportStmt ::= ("import" | "импорт") ImportTarget ";"
             | ("from" | "из") StringLiteral ("import" | "импорт") ImportList ";" ;

ImportTarget ::= StringLiteral (("as" | "как") Identifier)?
               | "{" ImportList "}" ("from" | "из") StringLiteral
               | "*" ("as" | "как") Identifier ("from" | "из") StringLiteral ;

ImportList ::= ImportItem ("," ImportItem)* ","? ;
ImportItem ::= Identifier (("as" | "как") Identifier)? ;

ScopeModifier ::= "line" | "строка" | "local" | "локальный" | "game" | "игра" | "save" | "сохранение" ;

VarDeclStmt ::= ("export" | "экспорт")? ScopeModifier? ("var" | "перем" | "пусть") VarDeclItem ("," VarDeclItem)* ("=" Expr)? ";" ;
VarDeclItem ::= Identifier (":" TypeRef)? ;

ConstDeclStmt ::= ("export" | "экспорт")? ("const" | "константа") Identifier (":" TypeRef)? "=" Expr ";" ;

Decorator ::= "@" Identifier ("(" (ArgumentList)? ")")? ;

FunctionDecl ::= Decorator* ("export" | "экспорт")? ("inline" | "встраиваемый")? 
                 ("function" | "функция" | "def" | "определение") 
                 Identifier GenericParams? "(" ParameterList? ")" ("->" TypeRef)? Block ;

ProcessDecl ::= Decorator* ("export" | "экспорт")? ("process" | "процесс") 
                Identifier "(" ParameterList? ")" Block ;

EventDecl ::= ("event" | "событие") "<" Identifier ">" Block ;

ClassDecl ::= Decorator* ("export" | "экспорт")? ("inline" | "встраиваемый")? 
              ("class" | "класс") Identifier GenericParams? 
              (("extends" | "расширяет") Identifier)? 
              (("implements" | "реализует") TypeList)? "{" ClassMember* "}" ;

ClassMember ::= VarDeclStmt | FunctionDecl ;

InterfaceDecl ::= ("export" | "экспорт")? ("interface" | "интерфейс") 
                  Identifier GenericParams? 
                  (("extends" | "расширяет") TypeList)? "{" InterfaceMember* "}" ;

InterfaceMember ::= ("function" | "функция" | "def") Identifier "(" ParameterList? ")" ("->" TypeRef)? ";" ;

EnumDecl ::= ("export" | "экспорт")? ("enum" | "перечисление") Identifier "{" EnumMemberList "}" ;
EnumMemberList ::= Identifier ("," Identifier)* ","? ;

TypeAliasDecl ::= ("export" | "экспорт")? ("type" | "typealias" | "тип" | "псевдоним") 
                  Identifier GenericParams? "=" TypeRef ";" ;

IfStmt ::= ("if" | "если") Expr Block (ElifBranch)* (ElseBranch)? ;
ElifBranch ::= ("elif" | "иначе_если" | "иначеесли") Expr Block ;
ElseBranch ::= ("else" | "иначе") Block ;

WhileStmt ::= (LabelDecl)? ("while" | "пока") Expr Block ;
ForStmt ::= (LabelDecl)? ("for" | "для") ForBinding ("in" | "в") Expr Block ;
ForBinding ::= Identifier ("," Identifier)? ;
LabelDecl ::= "'" Identifier ":" ;

MatchStmt ::= ("match" | "выбор") Expr "{" MatchArm* "}" ;
MatchArm ::= MatchPattern ("if" Expr)? "=>" (Block | Expr ("," | ";")) ;
MatchPattern ::= "_" | Expr ("|" Expr)* ;

TryCatchStmt ::= ("try" | "попытка") Block ("catch" | "перехват") ("(" Identifier ")")? Block ;
ReturnStmt ::= ("return" | "вернуть") Expr? ";" ;
BreakStmt ::= ("break" | "прервать") ("'" Identifier)? ";" ;
ContinueStmt ::= ("continue" | "продолжить") ("'" Identifier)? ";" ;

AssignStmt ::= AssignTarget AssignOp Expr ";" ;
AssignTarget ::= Identifier | Expr "." Identifier | Expr "[" Expr (":" Expr)? "]" ;
AssignOp ::= "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "^=" ;

Block ::= "{" Statement* "}" ;

Expr ::= TernaryExpr ;
TernaryExpr ::= LogicOrExpr ("?" Expr ":" Expr | ("if" | "если") LogicOrExpr ("else" | "иначе") Expr)? ;
LogicOrExpr ::= LogicAndExpr (("or" | "или" | "||") LogicAndExpr)* ;
LogicAndExpr ::= EqualityExpr (("and" | "и" | "&&") EqualityExpr)* ;
EqualityExpr ::= RelationalExpr (("==" | "!=") RelationalExpr)* ;
RelationalExpr ::= RangeExpr (("<" | "<=" | ">" | ">=" | ("in" | "в")) RangeExpr)* ;
RangeExpr ::= AdditiveExpr ((".." | "..=") AdditiveExpr)? ;
AdditiveExpr ::= MultiplicativeExpr (("+" | "-") MultiplicativeExpr)* ;
MultiplicativeExpr ::= PowerExpr (("*" | "/" | "%") PowerExpr)* ;
PowerExpr ::= UnaryExpr (("^" | "**") UnaryExpr)* ;
UnaryExpr ::= ("-" | "not" | "не" | "!" | "++" | "--") UnaryExpr | PostfixExpr ;

PostfixExpr ::= PrimaryExpr ("." Identifier | "::" Identifier ("<" Identifier ">")? | "[" Expr (":" Expr)? "]" | "(" ArgumentList? ")" | "++" | "--")* ;

PrimaryExpr ::= Literal
              | Identifier
              | LambdaExpr
              | ActionCallExpr
              | ValueExpr
              | "(" Expr ")"
              | PrimaryExpr ("as" | "как") TypeRef ;

ActionCallExpr ::= Identifier "::" Identifier ("<" Identifier ">")? "(" ArgumentList? ")" ;
ValueExpr ::= ("value" | "значение") "::" Identifier ("<" Identifier ">")? ;
LambdaExpr ::= "(" ParameterList? ")" "=>" (Expr | Block)
             | Identifier "=>" (Expr | Block) ;

TypeRef ::= Identifier ("<" TypeRef ("," TypeRef)* ">")? ;
GenericParams ::= "<" Identifier ("," Identifier)* ">" ;
TypeList ::= TypeRef ("," TypeRef)* ;
ParameterList ::= Parameter ("," Parameter)* ("," VariadicParameter)? | VariadicParameter ;
Parameter ::= ("ref" | "ссылка")? Identifier (":" TypeRef)? ("=" Expr)? ;
VariadicParameter ::= ("*" | "**") Identifier ;
ArgumentList ::= Argument ("," Argument)* ","? ;
Argument ::= (Identifier "=")? Expr ;
```

---

## 4. Type System and Static Semantics

### 4.1. Type Categories
The JustCode type system is static, nominal, with support for parametric polymorphism (generics) and automatic type inference.

#### 4.1.1. Primitive Types
- `number`: 64-bit IEEE-754 floating-point value (`f64`).
- `text`: UTF-8 text string.
- `boolean`: Boolean truth value (`true` / `false`).
- `any`: Dynamic supertype compatible with all types in assignments. Operations performed on `any` without an explicit `as` cast are strictly prohibited (`E0052`, `E0053`, `E0054`).
- `variable`: Reference to a physical JustMC variable slot.

#### 4.1.2. Collections and Data Structures
- `array<T>` (synonyms: `list<T>`, `массив<T>`, `список<T>`): Dynamic array of elements of type `T`.
- `map<K, V>` (synonyms: `dict<K, V>`, `словарь<K, V>`): Associative hash map.
- `Range`: Half-open numerical range `[start, end)`.
- `RangeInclusive`: Closed numerical range `[start, end]`.
- `Iterator<T>`: Iterator interface with methods `has_next() -> boolean` and `next() -> T`.
- `Iterable<T>`: Iterable collection interface with method `iter() -> Iterator<T>`.

#### 4.1.3. JustMC Platform Types
- `location`: 3D spatial coordinates and angles (`x, y, z, pitch, yaw`).
- `item`: Inventory item with count, material identifier, and NBT tags.
- `block`: Voxel world block representation.
- `entity`: World entity representation.
- `player`: Player entity (subtype of `entity`).
- `particle`: Visual particle effect.
- `potion`: Potion effect.
- `sound`: Sound effect.
- `vector`: 3D velocity/direction vector (`x, y, z`).

#### 4.1.4. Function Types
Callable objects are typed as `Fn<T1, T2, ..., R>` or `Function<T1, T2, ..., R>`.

### 4.2. Type Inference Algorithm
1. Type inference is based on Hindley-Milner unification.
2. In **Edition 2026**, all expressions MUST be fully resolved to concrete types. Unresolved type inference variables produce compilation error `E0057` (`UnresolvedTypeInference`).
3. Cyclic dependencies detected in the unification graph produce error `E0024` (`CyclicTypeInference`).

### 4.3. Type Casting: `as` Operator
The `as` operator performs static type coercion for the compiler. If the source type is identical to the target type, the compiler halts with error `E0058` (`RedundantCast`).

---

## 5. Memory Model and Scopes

The JustMC platform partitions variable storage into isolated scopes:

| Modifier | Prefix | Lifetime | Scope & Visibility |
|---|---|---|---|
| `line` | `` l`name` `` | Execution of current handler | Current line only. Not visible in callee sub-functions. Default in **Edition 2026** |
| `local` | `` l`name` `` | Lifetime of thread call stack | Shared across caller and callee sub-functions in the call stack. Default in **Edition 2023** |
| `game` | `` g`name` `` | Server session runtime | Shared across all players/handlers in the game world. Cleared on world restart |
| `save` | `` s`name` `` | Persistent database storage | Retained across server restarts indefinitely |

### 5.1. Compile-Time Constants (`inline var` / `const`)
Variables declared with the `inline` modifier or `const` keyword do not allocate memory slots on the JustMC server. Their values are folded at compile time (constant folding) directly into operation arguments.

### 5.2. Parameter Passing Semantics (`ref`)
1. By default, parameter passing is performed by value (shallow copy).
2. Parameters with the `ref` (or `ссылка`) modifier bind directly to the caller's variable reference. Mutations of the variable within the function immediately reflect in the caller's context.

---

## 6. Dunder Protocol and Operator Overloading

The JMCC compiler has no built-in binary operators. Every operator desugars into an invocation of the corresponding dunder method on a standard library class marked with `@lang_item`.

### 6.1. Complete Dunder Method Catalog

| Operator | Canonical Name | Supported Synonyms (En / Ru) | Signature |
|---|---|---|---|
| Constructor | `__init__` | `__конструктор__`, `__иниц__`, `__инициализатор__`, `__создать__` | `(self, ...) -> Self` |
| `a + b` | `__add__` | `__сложить__`, `__плюс__`, `__прибавить__` | `(self, other: T) -> R` |
| `a += b` | `__iadd__` | `__прибавить_присвоить__`, `__плюс_равно__`, `__присвоить_сложить__`, `__сложить_присвоить__` | `(self, other: T) -> Self` |
| `a - b` | `__subtract__` | `__вычесть__`, `__минус__`, `__отнять__`, `__sub__` | `(self, other: T) -> R` |
| `a -= b` | `__isubtract__` | `__вычесть_присвоить__`, `__минус_равно__`, `__присвоить_вычесть__`, `__isub__` | `(self, other: T) -> Self` |
| `a * b` | `__multiply__` | `__умножить__`, `__умножение__`, `__mul__` | `(self, other: T) -> R` |
| `a *= b` | `__imultiply__` | `__умножить_присвоить__`, `__умножить_равно__`, `__присвоить_умножить__`, `__imul__` | `(self, other: T) -> Self` |
| `a / b` | `__divide__` | `__разделить__`, `__поделить__`, `__деление__`, `__div__` | `(self, other: T) -> R` |
| `a /= b` | `__idivide__` | `__разделить_присвоить__`, `__разделить_равно__`, `__присвоить_разделить__`, `__idiv__` | `(self, other: T) -> Self` |
| `a % b` | `__remainder__` | `__остаток__`, `__остаток_от_деления__`, `__модуль__`, `__mod__` | `(self, other: T) -> R` |
| `a %= b` | `__iremainder__` | `__остаток_присвоить__`, `__присвоить_остаток__`, `__imod__` | `(self, other: T) -> Self` |
| `a ^ b`, `a ** b` | `__pow__` | `__степень__`, `__возвести_в_степень__` | `(self, other: T) -> R` |
| `a ^= b` | `__ipow__` | `__степень_присвоить__`, `__присвоить_степень__` | `(self, other: T) -> Self` |
| `a == b` | `__equals__` | `__равно__`, `__равенство__`, `__eq__` | `(self, other: T) -> boolean` |
| `a != b` | `__not_equals__` | `__не_равно__`, `__неравно__`, `__неравенство__`, `__ne__` | `(self, other: T) -> boolean` |
| `a > b` | `__greater__` | `__больше__`, `__gt__` | `(self, other: T) -> boolean` |
| `a < b` | `__less__` | `__меньше__`, `__lt__` | `(self, other: T) -> boolean` |
| `a >= b` | `__greater_or_equals__` | `__больше_или_равно__`, `__больше_равно__`, `__ge__` | `(self, other: T) -> boolean` |
| `a <= b` | `__less_or_equals__` | `__меньше_или_равно__`, `__меньше_равно__`, `__le__` | `(self, other: T) -> boolean` |
| `a in b` | `__contains__` | `__содержит__`, `__содержится__`, `__в__` | `(self, item: T) -> boolean` (called on `b`) |
| `a[i]` | `__subscript__` | `__индекс__`, `__элемент__`, `__взять__`, `__getitem__` | `(self, index: I) -> R` (`@getter`) |
| `a[i] = v` | `__subscript__` | `__setitem__` | `(self, index: I, value: V)` (`@setter`) |
| `a[i:j]` | `__slice__` | `__срез__` | `(self, start: number, end: number) -> R` (`@getter`) |
| `a[i:j] = v` | `__slice__` | `__срез__` | `(self, start: number, end: number, value: V)` (`@setter`) |
| `a & b` | `__bitand__` | `__bit_and__`, `__бит_и__`, `__побитовое_и__` | `(self, other: number) -> number` |
| `a \| b` | `__bitor__` | `__bit_or__`, `__бит_или__`, `__побитовое_или__` | `(self, other: number) -> number` |
| `a ^ b` (bit) | `__bitxor__` | `__bit_xor__`, `__бит_искл_или__`, `__побитовое_искл_или__` | `(self, other: number) -> number` |
| `a << b` | `__lshift__` | `__shl__`, `__сдвиг_влево__` | `(self, bits: number) -> number` |
| `a >> b` | `__rshift__` | `__shr__`, `__сдвиг_вправо__` | `(self, bits: number) -> number` |
| `a and b` | `__and__` | `__и__` | `(self, other: boolean) -> boolean` |
| `a or b` | `__or__` | `__или__` | `(self, other: boolean) -> boolean` |
| `not a` | `__not__` | `__не__` | `(self) -> boolean` |
| `-a` | `__neg__` | `__отрицание__`, `__минус_унарный__` | `(self) -> Self` |
| `a.prop = v` | `__set_attribute__` | `__установить_атрибут__`, `__задать_атрибут__` | `(self, name: text, val: any)` |
| `a.prop` | `__get_attribute__` | `__получить_атрибут__`, `__взять_атрибут__` | `(self, name: text) -> any` |
| `${a}` | `__str__` | `__строка__`, `__текст__`, `__text__` | `(self) -> text` |
| `len(a)` | `__len__` | `__длина__`, `__размер__` | `(self) -> number` |
| `for x in a` | `__iter__` | `__итератор__`, `__итер__` | `(self) -> Iterator<T>` |
| `it.next()` | `__next__` | `__следующий__`, `__след__` | `(self) -> T` |

---

## 7. Control Flow Semantics

### 7.1. Branching
1. `if` conditions MUST evaluate to a boolean type (`E0014`).
2. `elif` branches are deprecated (`W0001`); developers are advised to use `match`.
3. Ternary operators `cond ? a : b` and `a if cond else b` require a boolean condition (`E0034`) and strictly identical types for both branches (`E0051`).

### 7.2. Loops and Iteration
1. **`while`**: Repeats execution while the condition evaluates to `true`.
2. **`for .. in`**:
   - `for x in coll`: Invocates `coll.__iter__()` followed by `.has_next()` and `.next()`.
   - `for idx, val in arr`: Iterates over array elements while generating indices.
   - `for key, val in map`: Iterates over key-value pairs.
3. Loop labels: `'label: while ...`, `break 'label;`, `continue 'label;`. An unknown label produces compilation error `E0030`.

### 7.3. Pattern Matching (`match`)
- Pattern alternatives using `|`: `case 1 | 2 | 3 => ...`.
- Pattern guards: `case x if x > 10 => ...`.
- Default wildcard: `_ => ...`.
- All `match` arms MUST return identical data types (`E0026`).

### 7.4. Exceptions
- The statement `throw <SEVERITY> <TEXT>` (`WARNING`, `ERROR`, `FATAL`).
- The construct `try { ... } catch (err) { ... }` lowers to platform JustMC `controller::catch_exception` blocks.

---

## 8. Functions, Closures, and Processes

### 8.1. Signatures and Parameters
- Default parameter values: `function fn(x: number = 0)`.
- Variadic arguments: `*args` (`array<any>`) and `**kwargs` (`map<text, any>`).
- `inline` modifier: Enforces inlining of the function body at call sites.

### 8.2. Processes (`process`)
- Asynchronous coroutines executed in isolated JustMC threads.
- Returning values via `return <expr>` is prohibited (`E0060`). Process names cannot be used in expression positions (`E0056`).

### 8.3. Formal Semantics of Lambda Lifting
Anonymous lambda functions `(args) => body` are transformed by the `lift_lambdas` compiler pass as follows:
1. The lambda body is lifted into a synthetic global inline function `__lambda_{counter}(args)`.
2. A synthetic inline class `__LambdaClass_{counter}` is generated, implementing the `Function` interface and providing a `call(self, args) -> R` method that delegates to `__lambda_{counter}`.
3. The original lambda expression in the AST is replaced with an instantiation of this class: `__LambdaClass_{counter}()`.

### 8.4. Metadata Decorators

| Decorator | Target Declaration | Purpose |
|---|---|---|
| `@alias("name")` | Function, class, enum, process | Registers a global alias |
| `@alias(en = "...", ru = "...")` | Any declaration | Explicitly specifies English and Russian names |
| `@getter` / `@setter` | Class method | Marks property accessor or indexer |
| `@lang_item` | Standard library class | Registers system operator class in `IrCtx` |
| `@dict` | Class | Class serialized as an associative dictionary |
| `@test` | Function | Registers function as an executable unit test |
| `@should_panic` | Test function | Expects test execution to terminate with an error |
| `@should_panic(expected = "...")` | Test function | Expects panic matching the specified substring |
| `@ignore("reason")` | Test function | Excludes test from automated test runner execution |
| `@item("material")` | Function / process | Icon representation in JustMC GUI |
| `@description("...")` | Function / process | Description displayed in server menus |
| `@args(param = "...")` | Function | Parameter documentation for action in editor |
| `@hidden` | Function | Hides function from public JustMC editor catalog |
| `@overload` | Function | Marks function signature overload variant |

---

## 9. Platform Integration with JustMC

### 9.1. Action Invocations
Syntax: `category::action<selector>(arguments);`.
Categories are strictly validated against the `jmcdata` schema:
- `player` — Player manipulation.
- `entity` — Mob and entity manipulation.
- `world` — Block, lighting, weather, and time modification.
- `variable` — Operations on memory registers and collections.
- `code` — Subroutine calls, process spawning, delays (`code::wait`).
- `control` — Platform loop and conditional controllers.

### 9.2. Selector Partitioning by Action Target

| Action Category / Context | Valid Selectors |
|---|---|
| **Player Actions (`player`)** | `current`, `default_player`, `killer_player`, `damager_player`, `shooter_player`, `victim_player`, `random_player`, `all_players` |
| **Entity Actions (`entity`)** | `current`, `default_entity`, `killer_entity`, `damager_entity`, `shooter_entity`, `projectile`, `victim_entity`, `random_entity`, `all_mobs`, `all_entities`, `last_entity` |
| **Game Values (`game_value`)** | `current`, `default`, `default_entity`, `killer_entity`, `damager_entity`, `victim_entity`, `shooter_entity`, `projectile`, `last_entity` |

### 9.3. Events and Cancellation
- Event handlers are declared via `event<event_name> { ... }`.
- Each event in the `jmcdata` schema specifies `cancellable: true/false`. Cancellable events can be aborted by invoking the platform action `event::cancel()`.
- Top-level code outside of event handlers is grouped into a synthetic `world_start` handler.
- Every handler body is unconditionally wrapped by the compiler in `controller_measure_time` instrumentation.

---

## 10. Standard Library (`std/`)

The standard library is bundled with the compiler and automatically imported via the system prelude:
- In **Edition 2026**: `std/prelude_2026.jc` (imports `primitives` and `math/core.jc`).
- In **Edition 2023**: `std/prelude_2023.jc` (imports `primitives`).

### 10.1. Standard Library Module Tree

```text
std/
├── prelude_2023.jc
├── prelude_2026.jc
├── primitives/
│   ├── code/
│   │   ├── any.jc          # Base supertype
│   │   ├── array.jc        # Implementation of array<T> and list methods
│   │   ├── boolean.jc      # Boolean type
│   │   ├── function.jc     # Function and Fn interfaces
│   │   ├── iterator.jc     # Iterator, Iterable, Range, RangeInclusive
│   │   ├── map.jc          # Implementation of map<K, V>
│   │   ├── number.jc       # 64-bit arithmetic and bitwise operations
│   │   ├── text.jc         # String methods, slices, concatenation
│   │   ├── value.jc        # Platform dynamic values
│   │   ├── variable.jc     # Direct memory addressing
│   │   └── vector.jc       # 3D vectors
│   └── world/
│       ├── block.jc, entity.jc, item.jc, location.jc
│       ├── particle.jc, player.jc, potion.jc, sound.jc, world.jc
├── math/
│   ├── core.jc             # Mathematical functions (sqrt, sin, cos, abs, clamp)
│   └── matrix.jc           # Matrix computations
├── ai/
│   ├── astar.jc            # A* pathfinding algorithm
│   └── nn.jc               # Neural network layers and matrix weights
├── effects/
│   └── text/
│       ├── bubble.jc       # Overhead bubble text messages
│       ├── dialog.jc       # Interactive selection dialogs
│       └── leaderboard.jc  # Holographic leaderboards
└── security/
    └── modchecker.jc       # Client mod verification
```

### 10.2. `Range` and `RangeInclusive` Method Specifications
Range classes implement the `Iterable<number>` and `Iterator<number>` contracts:
- `Range(start: number, end: number, step: number = 1)` — Half-open interval `[start, end)`.
- `RangeInclusive(start: number, end: number, step: number = 1)` — Closed interval `[start, end]`.
- `.has_next() -> boolean` — Checks if another number is available.
- `.next() -> number` — Increments the current value by `step` and returns it.
- `.iter() -> Self` — Returns the iterator instance.
- `.step_by(step: number) -> Self` — Sets the step offset.
- `.len() -> number` — Computes the number of elements in the range accounting for step.
- `.contains(val: number) -> boolean` — Checks if a number falls within the range.
- `.to_array() -> array<number>` — Generates an array of numbers from the range.

---

## 11. Compiler Architecture and Optimization Pipeline

The compilation pipeline operates strictly sequentially:

```
Source Files (.jc)
  │
  ▼ [1. ast::parse_file] ────── Lexer (Logos), Pratt parser, import/from/export resolution
  │
  ▼ [2. ast::lift_lambdas] ──── Lambda Lifting: extracts lambdas to synthetic functions & classes
  │
  ▼ [3. IrCtx::new] ────────── Cross-phase symbol table construction (classes, enums, inline functions)
  │
  ▼ [4. ast::analyze] ──────── Semantic analysis, Hindley-Milner Unifier, strict type checking
  │
  ▼ [5. ir::hir::ast_to_hir] ── Lowers AST to High-Level IR (egg::RecExpr<Hir>)
  │
  ▼ [6. HIR Passes] ────────── HIR optimizations (module passes & fixpoint function passes)
  │
  ▼ [7. ir::hir_expand] ────── Overload expansion into explicit standard library dunder calls
  │
  ▼ [8. ir::mir::lower_to_mir] Lowers to Mid-Level IR (egg::RecExpr<Mir>)
  │
  ▼ [9. MIR Passes] ────────── MIR optimizations (instruction and register coalescing)
  │
  ▼ [10. ir::codegen] ──────── Object tree construction (jmcdata::module::Module)
  │
  ▼ [11. serde_json] ───────── Serialization to final compact or formatted JSON
```

### 11.1. Optimization Pass Identifiers for CLI
Pass names for the `--passes` and `--disable-passes` flags are formed by truncating the enum variant name at `(` and lowercasing:

#### HIR Module Passes:
- `testhirmodulepass` — Test module pass.
- `inline` — Function inlining (`inliner::InlinePass`).
- `deadfunctionelimination` — Elimination of uncalled functions (`keep_tests` retains `@test` when `--test` is active).

#### HIR Function Passes (iterated up to 3 rounds until fixpoint):
- `noop` — No-op pass.
- `constantfolding` — Constant folding in `egg` e-graph engine.
- `algebraicsimplification` — Algebraic simplifications in e-graph.
- `copypropagation` — Copy propagation across variables.
- `deadcodeelimination` — Elimination of unreachable code.

#### MIR Function Passes:
- `redundantreturnelimination` — Elimination of redundant trailing `return` operations.
- `copycoalescing` — Coalescing intermediate register copies.
- `setvariablefolding` — Folding sequential variable assignments.
- `coordinatefolding` — Folding constant coordinate computations for locations and vectors.
- `redundantelseelimination` — Elimination of empty and redundant `else` branches.

### 11.2. Output JSON Format Specification (`jmcdata::module::Module`)
The root JSON document contains a `handlers` array of `Line` objects:

```json
{
  "handlers": [
    {
      "type": "event",
      "position": 0,
      "event": "player_join",
      "operations": [
        {
          "action": "player_message",
          "values": [
            {
              "slot": "text",
              "value": {
                "type": "text",
                "text": "Welcome!",
                "parsing": "legacy"
              }
            }
          ]
        }
      ]
    }
  ]
}
```

- `Line.type`: `"event"` | `"process"` | `"function"`.
- `Line.position`: Sequential line index on the platform (`u16`).
- `LineValue`: For events, `event: EventId`; for functions and processes, `name: Cow<str>` and `values: LiteMap`.
- `Op`: Contains `action: ActionId`, `values: LiteMap`, optional nested `operations` block, `conditional`, `selection`, `is_inverted`.
- Variants of `Value`:
  - `number`: `{ "type": "number", "number": 42.0 }`
  - `text`: `{ "type": "text", "text": "...", "parsing": "legacy" | "plain" | "minimessage" | "json" }`
  - `variable`: `{ "type": "variable", "variable": "name", "scope": "line" | "local" | "unsaved" | "saved" }`
  - `location`: `{ "type": "location", "x": 0.0, "y": 64.0, "z": 0.0, "yaw": 0.0, "pitch": 0.0 }`
  - `vector`: `{ "type": "vector", "x": 0.0, "y": 1.0, "z": 0.0 }`
  - `game_value`: `{ "type": "game_value", "game_value": "health", "selection": "{\"type\":\"current\"}" }`
  - `item`: `{ "type": "item", "item": "diamond_sword" }`
  - `array`: `{ "type": "array", "values": [ ... ] }`
  - `map`: `{ "type": "map", "values": { ... } }`

### 11.3. JustMC Hardware Platform Limits (`jmcdata::consts`)
- `MAX_HANDLERS_PER_FLOOR = 23`: Maximum handlers per plot floor.
- `MAX_FLOORS = 15`: Maximum number of plot floors.
- `MAX_HANDLERS = 345`: Absolute limit of handlers in a single world.

---

## 12. Test Runner

### 12.1. Declaring Tests
Test functions are annotated with the `@test` decorator:
```jc
@test
function test_addition() {
    var a = 2 + 2;
    if a != 4 { throw ERROR "Arithmetic failure"; }
}

@test
@should_panic(expected = "Division by zero")
function test_failure() {
    var x = 10 / 0;
}

@test
@ignore("Network module under development")
function test_wip() {}
```

### 12.2. Running Tests
```bash
jmcc test [filter] [flags]
```
- `--exact` — Exact filter match on test names.
- `--ignored` — Run only tests marked with `@ignore`.
- `--nocapture` — Direct stdout/stderr output without buffering.
- `--locale <ru|en>` — Test report language.
- `--locked` — Enforces exact `jmcc.lock` dependencies.
- `--offline` — Disables network access.

---

## 13. Diagnostic Error Code Catalog

### 13.1. Semantic Errors (`E0001` .. `E0060`)

| Code | Identifier | Description |
|---|---|---|
| `E0001` | `InternalCompilerError` | Internal compiler error (ICE / Internal Compiler Error) |
| `E0002` | `UnknownAction` | Unknown JustMC platform action |
| `E0003` | `UnknownMethod` | Unknown method on class |
| `E0004` | `UnknownProperty` | Unknown property on struct or class |
| `E0005` | `UndeclaredVariable` | Variable not declared in current scope |
| `E0006` | `TypeMismatchVarDecl` | Cannot assign value of this type to variable |
| `E0007` | `TypeMismatchAssign` | Type mismatch in assignment statement |
| `E0008` | `TypeMismatchReturn` | Return value does not match function signature |
| `E0009` | `DuplicateFunction` | Duplicate function declaration with matching name |
| `E0010` | `DuplicateProcess` | Duplicate process declaration with matching name |
| `E0011` | `BreakOutsideLoop` | `break` statement outside loop body |
| `E0012` | `ReturnOutsideCallable` | `return` statement outside function or process |
| `E0013` | `MissingReturnValue` | Function with return type must return a value |
| `E0014` | `InvalidCondition` | Condition must be a boolean expression |
| `E0015` | `InvalidNumericOperand` | Unary operator requires numeric operand |
| `E0016` | `InvalidArithmetic` | Arithmetic operation on non-numeric operands |
| `E0017` | `UnknownParam` | Unknown named parameter in invocation |
| `E0018` | `TooManyArgs` | Exceeded maximum number of arguments |
| `E0019` | `MissingArgument` | Missing required function argument |
| `E0020` | `FuncArgTypeMismatch` | Argument type mismatch with function parameter |
| `E0021` | `ActionArgTypeMismatch` | Argument type mismatch with JustMC action schema |
| `E0022` | `InvalidSelector` | Invalid selector for this action |
| `E0023` | `CyclicInheritance` | Cyclic class inheritance detected |
| `E0024` | `CyclicTypeInference` | Dependency cycle detected during type inference |
| `E0025` | `NotIterable` | Type does not support iteration via `for` (missing `__iter__`) |
| `E0026` | `MatchArmTypeMismatch` | `match` arms return incompatible data types |
| `E0027` | `MissingInterfaceMethod` | Class does not implement required interface method |
| `E0028` | `InterfaceMethodSignatureMismatch` | Class method signature does not match interface |
| `E0029` | `AlreadyDeclared` | Identifier already declared in current scope |
| `E0030` | `UnknownLoopLabel` | Unknown loop label in `break` or `continue` statement |
| `E0031` | `CompoundAssignRhs` | Right-hand side of compound assignment requires number |
| `E0032` | `CompoundAssignTarget` | Target of compound assignment must be numeric |
| `E0033` | `InvalidElifCondition` | `elif` condition must be a boolean expression |
| `E0034` | `InvalidTernaryCondition` | Ternary operator condition must be boolean |
| `E0035` | `InvalidNotOperand` | Operator `!` requires boolean operand |
| `E0036` | `InvalidBitwise` | Bitwise operation requires numeric operands |
| `E0037` | `InvalidLogical` | Logical operation requires boolean operands |
| `E0038` | `InvalidSlice` | Slice operator `[:]` not supported for this type |
| `E0039` | `InvalidSubscript` | Indexing `[]` not supported for this type |
| `E0040` | `UnknownParent` | Parent class not found |
| `E0041` | `UnknownConstructor` | Unknown constructor during instantiation |
| `E0042` | `CtorArgTypeMismatch` | Constructor named argument type mismatch |
| `E0043` | `CtorPositionalArgTypeMismatch` | Constructor positional argument type mismatch |
| `E0044` | `FuncPositionalArgTypeMismatch` | Function positional argument type mismatch |
| `E0045` | `InvalidEnumValue` | Invalid enum member value |
| `E0046` | `EnumIndexOutOfBounds` | Enum member index out of bounds |
| `E0047` | `InvalidBoolEnum` | Expected boolean enum value |
| `E0048` | `UnknownGameValue` | Unknown JustMC platform game value |
| `E0049` | `InfinitePropertyRecursion` | Infinite recursion during property access |
| `E0050` | `UnknownType` | Unknown data type |
| `E0051` | `TernaryBranchTypeMismatch` | Type mismatch between `then` and `else` branches |
| `E0052` | `OperationOnAny` | Binary operation on `any` value without `as` |
| `E0053` | `PropertyAccessOnAny` | Property access on `any` value without `as` |
| `E0054` | `MethodCallOnAny` | Method call on `any` value without `as` |
| `E0055` | `VoidReturnValueUsed` | Value of void function used in expression |
| `E0056` | `ProcessReturnValueUsed` | Process does not return a value and cannot be an expression |
| `E0057` | `UnresolvedTypeInference` | Failed to infer expression type (Edition 2026) |
| `E0058` | `RedundantCast` | Redundant cast: value already belongs to type |
| `E0059` | `CyclicInterfaceInheritance` | Cyclic interface inheritance detected |
| `E0060` | `ReturnFromProcess` | Process cannot return a value via `return <expr>` |

### 13.2. Compiler Warnings (`W0001`)

| Code | Identifier | Description |
|---|---|---|
| `W0001` | `DeprecatedElif` | Use of `elif` is deprecated (use `match`) |

### 13.3. Parser Syntax Errors (`S0001` .. `S0005`)

| Code | Identifier | Description |
|---|---|---|
| `S0001` | `InternalParserError` | Internal parser error (ICE parser / Internal Parser Error) |
| `S0002` | `UnexpectedToken` | Unexpected token during parsing |
| `S0003` | `UnexpectedEof` | Unexpected end of file in syntax construct |
| `S0004` | `NumberParse` | Failed to parse literal into 64-bit number |
| `S0005` | `GenericParseError` | Generic syntax parse error |

---

## 14. Command Line Interface (CLI)

```bash
jmcc <command> [arguments]
```

### 14.1. Subcommand `compile`
Compiles source code into an executable JustMC JSON module.
```bash
jmcc compile [input] [-o output.json] [-O 0..3] [--release] [--edition 2026] [--emit ast,hir,mir,json]
```

- `input`: Path to `.jc` file, directory containing `jmcc.toml`, or omitted (builds current project).
- `-o, --output <PATH>`: Output file path.
- `-O, --opt-level <0..3>`: Optimization level (default: `2`).
- `--profile <NAME>`: Build profile from `jmcc.toml`.
- `--release`: Shortcut for `--profile release` (`-O 3`, `--emit json`).
- `--target <TARGET>`: Target compilation platform (default: `justmc`).
- `--edition <2023|2026>`: Language edition (default: `2026`).
- `--passes <LIST>`: Comma-separated list of explicitly enabled optimization passes.
- `--disable-passes <LIST>`: Comma-separated list of disabled optimization passes.
- `--emit <ast,hir,mir,json>`: Comma-separated list of generated artifact files.
- `--locale, --lang <ru|en>`: Diagnostic output language.
- `--locked`: Forbids updating `jmcc.lock`.
- `--offline`: Disables network requests.
- `--test`: Compiles while preserving test functions.

### 14.2. Subcommand `test`
```bash
jmcc test [filter] [--exact] [--ignored] [--nocapture] [--locale ru|en] [--locked] [--offline]
```

### 14.3. Subcommand `update`
```bash
jmcc update [package_name]
```

### 14.4. Subcommand `format`
```bash
jmcc format <path_to_file_or_directory> [--check]
```

### 14.5. Exit Codes
- `0`: Success.
- `1`: Compilation error, semantic analysis failure, test failure, or formatting mismatch under `--check`.
- `101`: Critical internal process error (Panic / Fatal IO).

### 14.6. Internal Compiler Error Handling (Panic Handler / ICE)
When an unexpected runtime error (panic) occurs within the compiler, the `jmcc::panic_handler` hook automatically activates:
1. Captures a complete state snapshot (`PanicReport`):
   - Panic message and source location in Rust code (`file:line:col`).
   - Forcibly captured full backtrace (`Backtrace::force_capture()`).
   - JMCC compiler version, target architecture, and operating system.
   - Command-line arguments and current working directory.
   - UTC timestamp.
2. Dumps the crash report to disk: `jmcc-crash-<timestamp>-<pid>.log` in the current working directory (or in the system temporary directory if the working directory is not writable).
3. Prints a localized diagnostic message with error code `E0001` to `stderr` (in Russian or English based on environment or `--locale` / `--lang` flags), including an issue tracker link:
   `https://github.com/jmcc-reborn/jmcc/issues`
4. Exits the process with status code `101`.
