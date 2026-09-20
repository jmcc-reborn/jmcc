//! Semantic analysis: types, symbol tables, and type-checking entry point.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::ir::KNOWN_OBJECTS;
use crate::ir::ctx::{ClassInfo, IrCtx};
use crate::utils::resolve_action_arg_def;
use text_size::TextSize;
use tracing::{info, instrument, trace, warn};

use crate::i18n::{Lang, current_lang, plural_errors, plural_params};

mod actions;
mod calls;
mod decls;
mod exprs;
mod scopes;
mod stmts;
mod types;

/// Actions with a syntactic equivalent in the language.
///
/// Fields are `(object, method, how it is written, what the raw form loses)`. Warned about
/// rather than rejected: the raw form is sometimes the only way to reach what the syntax
/// does not cover.
const RAW_ACTION_FORMS: &[(&str, &str, &str, &str)] = &[
    ("variable", "add", "a + b", OPERATOR_NOTE),
    ("variable", "subtract", "a - b", OPERATOR_NOTE),
    ("variable", "multiply", "a * b", OPERATOR_NOTE),
    ("variable", "divide", "a / b", OPERATOR_NOTE),
    ("variable", "remainder", "a % b", OPERATOR_NOTE),
    ("variable", "pow", "a ^ b", OPERATOR_NOTE),
    ("variable", "increment", "a++", OPERATOR_NOTE),
    ("variable", "decrement", "a--", OPERATOR_NOTE),
    ("variable", "equals", "a == b", OPERATOR_NOTE),
    ("variable", "not_equals", "a != b", OPERATOR_NOTE),
    ("variable", "less", "a < b", OPERATOR_NOTE),
    ("variable", "less_or_equals", "a <= b", OPERATOR_NOTE),
    ("variable", "greater", "a > b", OPERATOR_NOTE),
    ("variable", "greater_or_equals", "a >= b", OPERATOR_NOTE),
    (
        "variable",
        "bitwise_operation",
        "a & b, a | b, a << b, a >> b",
        OPERATOR_NOTE,
    ),
    ("code", "call_function", "f(...)", CALL_NOTE),
    ("code", "start_process", "process_name(...)", CALL_NOTE),
];

/// `+`, `==`, … are methods of a `@lang_item` class, not the action itself.
const OPERATOR_NOTE: &str =
    "the raw form bypasses the `@lang_item` class that defines the operator";

/// The name is not resolved where it is written, so a typo only shows up at runtime.
const CALL_NOTE: &str = "the raw form passes the name as text, so nothing checks that it exists";

pub type DefId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Class(DefId, Vec<Type>),
    Enum(DefId),
    InferVar(usize),
    Param(StrId),
    Never,
    Unknown,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Class(id, args) => {
                if args.is_empty() {
                    write!(f, "Class#{id}")
                } else {
                    write!(f, "Class#{id}<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    write!(f, ">")
                }
            }
            Type::Enum(id) => write!(f, "Enum#{id}"),
            Type::InferVar(id) => write!(f, "?{id}"),
            Type::Param(id) => write!(f, "Param({id:?})"),
            Type::Never => f.write_str("Never"),
            Type::Unknown => f.write_str("Error"),
        }
    }
}

impl Type {
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

struct Unifier {
    parents: Vec<Type>,
    had_cycle: bool,
}

#[derive(Clone, Copy)]
struct ActionCallKind {
    is_method: bool,
    is_statement: bool,
}

impl Unifier {
    const fn new() -> Self {
        Self {
            parents: Vec::new(),
            had_cycle: false,
        }
    }

    fn new_var(&mut self) -> Type {
        let id = self.parents.len();
        self.parents.push(Type::InferVar(id));
        Type::InferVar(id)
    }

    #[instrument(skip(self), level = "trace")]
    fn find(&mut self, ty: &Type) -> Type {
        let mut visited = HashSet::new();
        self.find_inner(ty, &mut visited)
    }

    fn find_inner(&mut self, ty: &Type, visited: &mut HashSet<usize>) -> Type {
        match ty {
            Type::InferVar(id) => {
                if *id >= self.parents.len() {
                    return ty.clone();
                }
                if !visited.insert(*id) {
                    self.had_cycle = true;
                    return Type::Unknown;
                }
                let parent = self.parents[*id].clone();
                if parent == *ty {
                    ty.clone()
                } else {
                    let root = self.find_inner(&parent, visited);
                    self.parents[*id] = root.clone();
                    root
                }
            }
            Type::Class(id, args) => Type::Class(
                *id,
                args.iter().map(|t| self.find_inner(t, visited)).collect(),
            ),
            _ => ty.clone(),
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn unify(&mut self, t1: &Type, t2: &Type) -> Result<Type, String> {
        let t1 = self.find(t1);
        let t2 = self.find(t2);
        if t1 == t2 {
            return Ok(t1);
        }
        match (&t1, &t2) {
            (Type::Unknown, _) | (_, Type::Unknown) => Ok(Type::Unknown),
            (Type::Never, _) | (_, Type::Never) => Ok(t1),

            (Type::Param(p1), Type::Param(p2)) if p1 == p2 => Ok(t1.clone()),
            (Type::Param(_), other) | (other, Type::Param(_)) => {
                if matches!(other, Type::InferVar(_)) {
                    return Err(format!("Cannot unify {t1} and {t2}"));
                }
                Ok(other.clone())
            }

            (Type::InferVar(id), other) | (other, Type::InferVar(id)) => {
                self.parents[*id] = other.clone();
                Ok(other.clone())
            }
            (Type::Class(id1, args1), Type::Class(id2, args2)) => {
                if id1 != id2 {
                    return Err(format!("Cannot unify {t1} and {t2}"));
                }
                if args1.len() != args2.len() {
                    return Err("Generic count mismatch".to_owned());
                }
                let mut new_args = Vec::new();
                for (a1, a2) in args1.iter().zip(args2.iter()) {
                    new_args.push(self.unify(a1, a2)?);
                }
                Ok(Type::Class(*id1, new_args))
            }
            _ => Err(format!("Cannot unify {t1} and {t2}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub ty: Type,
    pub spread: u8,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
pub enum Symbol {
    Var {
        ty: Type,
        scope: VarScope,
        /// Type comes from an annotation (`var x: number`), not from the assigned value.
        declared: bool,
    },
    Param {
        ty: Type,
    },
    Func {
        params: Vec<ParamInfo>,
        return_type: Option<Type>,
        span: Span,
    },
    Proc {
        params: Vec<ParamInfo>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum SemanticErrorKind {
    InternalCompilerError {
        details: String,
    },
    BreakOutsideLoop,
    UnknownLoopLabel {
        name: String,
    },
    AlreadyDeclared {
        name: String,
    },
    DuplicateFunction {
        name: String,
        prev_span: String,
    },
    DuplicateProcess {
        name: String,
        prev_span: String,
    },
    TypeMismatchVarDecl {
        expected: String,
        actual: String,
    },
    TypeMismatchAssign {
        target: String,
        actual: String,
    },
    CompoundAssignRhs {
        op: String,
        actual: String,
    },
    CompoundAssignTarget {
        op: String,
        target: String,
    },
    ReturnOutsideCallable,
    TypeMismatchReturn {
        expected: String,
        actual: String,
    },
    ReturnFromProcess,
    MissingReturnValue,
    InvalidCondition {
        actual: String,
    },
    InvalidElifCondition {
        actual: String,
    },
    InvalidTernaryCondition {
        actual: String,
    },
    InvalidNotOperand {
        actual: String,
    },
    InvalidNumericOperand {
        op: String,
        actual: String,
    },
    InvalidArithmetic {
        op: String,
        lty: String,
        rty: String,
    },
    InvalidBitwise {
        op: String,
        lty: String,
        rty: String,
    },
    InvalidLogical {
        op: String,
        lty: String,
        rty: String,
    },
    InvalidSlice {
        ty: String,
    },
    InvalidSubscript {
        ty: String,
    },
    UnknownMethod {
        class: String,
        method: String,
        suggestion: String,
    },
    UnknownAction {
        object: String,
        name: String,
        suggestion: String,
    },
    UnknownProperty {
        ty: String,
        property: String,
        suggestion: String,
    },
    UnknownParent {
        parent: String,
    },
    UnknownConstructor {
        name: String,
    },
    CtorArgTypeMismatch {
        ctor: String,
        arg: String,
        expected: String,
        actual: String,
    },
    CtorPositionalArgTypeMismatch {
        ctor: String,
        idx: usize,
        expected: String,
        actual: String,
    },
    UnknownParam {
        name: String,
        arg: String,
        suggestion: String,
    },
    TooManyArgs {
        name: String,
        expected: usize,
        actual: usize,
    },
    MissingArgument {
        name: String,
        arg: String,
        suggestion: String,
    },
    FuncArgTypeMismatch {
        name: String,
        arg: String,
        expected: String,
        actual: String,
    },
    FuncPositionalArgTypeMismatch {
        name: String,
        idx: usize,
        expected: String,
        actual: String,
    },
    InvalidEnumValue {
        value: String,
        expected: String,
    },
    EnumIndexOutOfBounds {
        idx: usize,
        max: usize,
    },
    InvalidBoolEnum {
        expected: String,
    },
    UnknownGameValue {
        name: String,
    },
    InvalidSelector {
        selector: String,
        object: String,
    },
    ActionArgTypeMismatch {
        name: String,
        arg: String,
        expected: String,
        actual: String,
    },
    InfinitePropertyRecursion {
        property: String,
    },
    CyclicInheritance {
        class: String,
    },
    CyclicTypeInference,
    UnknownType(String),
    UndeclaredVariable {
        name: String,
        suggestion: String,
    },
    NotIterable {
        ty: String,
    },
    MatchArmTypeMismatch {
        expected: String,
        actual: String,
    },
    TernaryBranchTypeMismatch {
        then_ty: String,
        else_ty: String,
    },
    OperationOnAny {
        op: String,
    },
    PropertyAccessOnAny {
        property: String,
    },
    MethodCallOnAny {
        method: String,
    },
    VoidReturnValueUsed {
        name: String,
    },
    ProcessReturnValueUsed {
        name: String,
    },
    UnresolvedTypeInference,
    RedundantCast {
        ty: String,
    },
    MissingInterfaceMethod {
        class: String,
        interface: String,
        method: String,
    },
    InterfaceMethodSignatureMismatch {
        class: String,
        interface: String,
        method: String,
        details: String,
    },
    CyclicInterfaceInheritance {
        interface: String,
    },
    DeprecatedElif,
    EventNotCancellable {
        event: String,
    },
}

impl SemanticErrorKind {
    #[must_use]
    pub const fn is_warning(&self) -> bool {
        matches!(
            self,
            Self::DeprecatedElif | Self::EventNotCancellable { .. }
        )
    }
}

impl std::error::Error for SemanticErrorKind {}

impl std::fmt::Display for SemanticErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format_localized(current_lang()))
    }
}

impl SemanticErrorKind {
    #[expect(
        clippy::too_many_lines,
        reason = "Localized formatting for all semantic error kinds"
    )]
    #[must_use]
    pub fn format_localized(&self, lang: Lang) -> String {
        match lang {
            Lang::Ru => match self {
                Self::InternalCompilerError { details } => {
                    format!("Внутренняя ошибка компилятора (ICE): {details}")
                }
                Self::BreakOutsideLoop => "'break' вне цикла".to_owned(),
                Self::UnknownLoopLabel { name } => format!("Неизвестная метка цикла: '{name}'"),
                Self::AlreadyDeclared { name } => format!("'{name}' уже объявлен в этой области видимости"),
                Self::DuplicateFunction { name, prev_span } => {
                    format!("Дублирующееся объявление функции: '{name}' (ранее объявлено в {prev_span})")
                }
                Self::DuplicateProcess { name, prev_span } => {
                    format!("Дублирующееся объявление процесса: '{name}' (ранее объявлено в {prev_span})")
                }
                Self::TypeMismatchVarDecl { expected, actual } => {
                    format!("Невозможно присвоить значение типа '{actual}' переменной типа '{expected}'")
                }
                Self::TypeMismatchAssign { target, actual } => {
                    format!("Невозможно присвоить значение типа '{actual}' цели типа '{target}'")
                }
                Self::CompoundAssignRhs { op, actual } => {
                    format!("Составное присваивание '{op}' требует числовой правой части, получено '{actual}'")
                }
                Self::CompoundAssignTarget { op, target } => {
                    format!("Составное присваивание '{op}' требует числовой цели, получено '{target}'")
                }
                Self::ReturnOutsideCallable => "'return' вне функции или процесса".to_owned(),
                Self::TypeMismatchReturn { expected, actual } => {
                    format!("Невозможно вернуть значение типа '{actual}' из функции с типом возврата '{expected}'")
                }
                Self::ReturnFromProcess => "Невозможно вернуть значение из процесса".to_owned(),
                Self::MissingReturnValue => "Функция с типом возврата должна возвращать значение".to_owned(),
                Self::InvalidCondition { actual } => {
                    format!("Условие должно быть логическим выражением, получено '{actual}'")
                }
                Self::InvalidElifCondition { actual } => {
                    format!("Условие elif должно быть логическим выражением, получено '{actual}'")
                }
                Self::InvalidTernaryCondition { actual } => {
                    format!("Условие тернарного оператора должно быть логическим выражением, получено '{actual}'")
                }
                Self::InvalidNotOperand { actual } => {
                    format!("'!' требует логический операнд, получено '{actual}'")
                }
                Self::InvalidNumericOperand { op, actual } => {
                    format!("Оператор '{op}' требует числовой операнд, получено '{actual}'")
                }
                Self::InvalidArithmetic { op, lty, rty } => {
                    format!("Арифметическая операция '{op}' требует числовые операнды, получены '{lty}' и '{rty}'")
                }
                Self::InvalidBitwise { op, lty, rty } => {
                    format!("Побитовая операция '{op}' требует числовые операнды, получены '{lty}' и '{rty}'")
                }
                Self::InvalidLogical { op, lty, rty } => {
                    format!("Логическая операция '{op}' требует логические операнды, получены '{lty}' и '{rty}'")
                }
                Self::InvalidSlice { ty } => {
                    format!("Невозможно использовать оператор среза '[:]' для типа '{ty}'")
                }
                Self::InvalidSubscript { ty } => {
                    format!("Невозможно использовать оператор индексации '[]' для типа '{ty}'")
                }
                Self::UnknownMethod { class, method, suggestion } => {
                    format!("Неизвестный метод '{method}' у класса '{class}'{suggestion}")
                }
                Self::UnknownAction { object, name, suggestion } => {
                    format!("Неизвестное действие '{object}::{name}'{suggestion}")
                }
                Self::UnknownProperty { ty, property, suggestion } => {
                    format!("Неизвестное свойство '{property}' у типа '{ty}'{suggestion}")
                }
                Self::UnknownParent { parent } => format!("Неизвестный родительский класс: '{parent}'"),
                Self::UnknownConstructor { name } => format!("Неизвестный конструктор: '{name}'"),
                Self::CtorArgTypeMismatch { ctor, arg, expected, actual } => {
                    format!("Аргумент '{arg}' конструктора '{ctor}' ожидает тип '{expected}', получено '{actual}'")
                }
                Self::CtorPositionalArgTypeMismatch { ctor, idx, expected, actual } => {
                    format!("Аргумент {idx} конструктора '{ctor}' ожидает тип '{expected}', получено '{actual}'")
                }
                Self::UnknownParam { name, arg, suggestion } => {
                    format!("Функция '{name}' не имеет параметра с именем '{arg}'{suggestion}")
                }
                Self::TooManyArgs { name, expected, actual } => {
                    let p = plural_params(*expected, lang);
                    format!("Функция '{name}' ожидает максимум {p}, получено {actual}")
                }
                Self::MissingArgument { name, arg, suggestion } => {
                    format!("Отсутствует аргумент '{arg}' для функции '{name}'{suggestion}")
                }
                Self::FuncArgTypeMismatch { name, arg, expected, actual } => {
                    format!("Аргумент '{arg}' функции '{name}' ожидает тип '{expected}', получено '{actual}'")
                }
                Self::FuncPositionalArgTypeMismatch { name, idx, expected, actual } => {
                    format!("Аргумент {idx} функции '{name}' ожидает тип '{expected}', получено '{actual}'")
                }
                Self::InvalidEnumValue { value, expected } => {
                    format!("Неверное значение перечисления '{value}'. Ожидалось одно из: {expected}")
                }
                Self::EnumIndexOutOfBounds { idx, max } => {
                    format!("Индекс перечисления {idx} вне диапазона. Максимальный индекс: {max}")
                }
                Self::InvalidBoolEnum { expected } => {
                    format!("Неверное булево значение для перечисления. Ожидалось одно из: {expected}")
                }
                Self::UnknownGameValue { name } => format!("Неизвестное игровое значение: {name}"),
                Self::InvalidSelector { selector, object } => {
                    format!("Неизвестный или неверный селектор: '{selector}' для объекта '{object}'")
                }
                Self::ActionArgTypeMismatch { name, arg, expected, actual } => {
                    format!("Аргумент '{arg}' действия '{name}' ожидает тип '{expected}', получено '{actual}'")
                }
                Self::InfinitePropertyRecursion { property } => {
                    format!("Обнаружена бесконечная рекурсия: обращение к свойству '{property}' у 'self' внутри его же геттера или сеттера")
                }
                Self::CyclicInheritance { class } => {
                    format!("Обнаружено циклическое наследование для класса '{class}'")
                }
                Self::CyclicTypeInference => {
                    "Обнаружен циклический вывод типов (вероятно, вызвано несоответствием типов или неверным приведением)".to_owned()
                }
                Self::UnknownType(s) => format!("Неизвестный тип: {s}"),
                Self::UndeclaredVariable { name, suggestion } => {
                    format!("Использование необъявленной переменной: '{name}'{suggestion}")
                }
                Self::NotIterable { ty } => {
                    format!("Тип '{ty}' не итерируем в цикле 'for' (ожидался массив или словарь)")
                }
                Self::MatchArmTypeMismatch { expected, actual } => {
                    format!("Ветви match имеют несовместимые типы: ветвь имеет тип '{actual}', но предыдущая ветвь вернула '{expected}'")
                }
                Self::TernaryBranchTypeMismatch { then_ty, else_ty } => {
                    format!("Ветви тернарного выражения имеют несовместимые типы: ветвь 'then' имеет тип '{then_ty}', а 'else' — '{else_ty}'")
                }
                Self::OperationOnAny { op } => {
                    format!("Невозможно выполнить бинарную операцию '{op}' над значением типа 'any'")
                }
                Self::PropertyAccessOnAny { property } => {
                    format!("Невозможно получить доступ к свойству '{property}' у значения типа 'any'")
                }
                Self::MethodCallOnAny { method } => {
                    format!("Невозможно вызвать метод '{method}' у значения типа 'any'")
                }
                Self::VoidReturnValueUsed { name } => format!("Функция '{name}' не возвращает значение"),
                Self::ProcessReturnValueUsed { name } => format!("Процесс '{name}' не возвращает значение"),
                Self::UnresolvedTypeInference => {
                    "Не удалось вывести тип выражения. Пожалуйста, добавьте явные аннотации типов.".to_owned()
                }
                Self::RedundantCast { ty } => format!("Избыточное приведение: значение уже имеет тип '{ty}'"),
                Self::MissingInterfaceMethod { class, interface, method } => {
                    format!("Класс '{class}' не реализует метод '{method}' из интерфейса '{interface}'")
                }
                Self::InterfaceMethodSignatureMismatch { class, interface, method, details } => {
                    format!("Сигнатура метода '{method}' в классе '{class}' не совпадает с интерфейсом '{interface}': {details}")
                }
                Self::CyclicInterfaceInheritance { interface } => {
                    format!("Обнаружено циклическое наследование интерфейса для '{interface}'")
                }
                Self::DeprecatedElif => {
                    "использование 'elif' не рекомендуется, используйте 'match'".to_owned()
                }
                Self::EventNotCancellable { event } => {
                    format!("Событие '{event}' не поддерживает отмену")
                }
            },
            Lang::En => match self {
                Self::InternalCompilerError { details } => {
                    format!("Internal compiler error (ICE): {details}")
                }
                Self::BreakOutsideLoop => "'break' outside of a loop".to_owned(),
                Self::UnknownLoopLabel { name } => format!("Unknown loop label: '{name}'"),
                Self::AlreadyDeclared { name } => format!("'{name}' is already declared in this scope"),
                Self::DuplicateFunction { name, prev_span } => {
                    format!("Duplicate function declaration: '{name}' (previously declared at {prev_span})")
                }
                Self::DuplicateProcess { name, prev_span } => {
                    format!("Duplicate process declaration: '{name}' (previously declared at {prev_span})")
                }
                Self::TypeMismatchVarDecl { expected, actual } => {
                    format!("Cannot assign value of type '{actual}' to variable of type '{expected}'")
                }
                Self::TypeMismatchAssign { target, actual } => {
                    format!("Cannot assign value of type '{actual}' to target of type '{target}'")
                }
                Self::CompoundAssignRhs { op, actual } => {
                    format!("Compound assignment '{op}' requires numeric right-hand side, got '{actual}'")
                }
                Self::CompoundAssignTarget { op, target } => {
                    format!("Compound assignment '{op}' requires numeric target, got '{target}'")
                }
                Self::ReturnOutsideCallable => "'return' outside of a function or process".to_owned(),
                Self::TypeMismatchReturn { expected, actual } => {
                    format!("Cannot return value of type '{actual}' from function with return type '{expected}'")
                }
                Self::ReturnFromProcess => "Cannot return a value from a process".to_owned(),
                Self::MissingReturnValue => "Function with return type must return a value".to_owned(),
                Self::InvalidCondition { actual } => format!("Condition must be truthy, got '{actual}'"),
                Self::InvalidElifCondition { actual } => format!("elif condition must be truthy, got '{actual}'"),
                Self::InvalidTernaryCondition { actual } => {
                    format!("Ternary condition must be truthy, got '{actual}'")
                }
                Self::InvalidNotOperand { actual } => format!("'!' requires truthy operand, got '{actual}'"),
                Self::InvalidNumericOperand { op, actual } => {
                    format!("Operator '{op}' requires numeric operand, got '{actual}'")
                }
                Self::InvalidArithmetic { op, lty, rty } => {
                    format!("Arithmetic operation '{op}' requires numeric operands, got '{lty}' and '{rty}'")
                }
                Self::InvalidBitwise { op, lty, rty } => {
                    format!("Bitwise operation '{op}' requires numeric operands, got '{lty}' and '{rty}'")
                }
                Self::InvalidLogical { op, lty, rty } => {
                    format!("Logical operation '{op}' requires truthy operands, got '{lty}' and '{rty}'")
                }
                Self::InvalidSlice { ty } => format!("Cannot use slice operator '[:]' on type '{ty}'"),
                Self::InvalidSubscript { ty } => format!("Cannot use subscript operator '[]' on type '{ty}'"),
                Self::UnknownMethod { class, method, suggestion } => {
                    format!("Unknown method '{method}' on class '{class}'{suggestion}")
                }
                Self::UnknownAction { object, name, suggestion } => {
                    format!("Unknown action '{object}::{name}'{suggestion}")
                }
                Self::UnknownProperty { ty, property, suggestion } => {
                    format!("Unknown property '{property}' on type '{ty}'{suggestion}")
                }
                Self::UnknownParent { parent } => format!("Unknown parent class: '{parent}'"),
                Self::UnknownConstructor { name } => format!("Unknown constructor: '{name}'"),
                Self::CtorArgTypeMismatch { ctor, arg, expected, actual } => {
                    format!("Constructor '{ctor}' argument '{arg}' expects type '{expected}', got '{actual}'")
                }
                Self::CtorPositionalArgTypeMismatch { ctor, idx, expected, actual } => {
                    format!("Constructor '{ctor}' argument {idx} expects type '{expected}', got '{actual}'")
                }
                Self::UnknownParam { name, arg, suggestion } => {
                    format!("Function '{name}' has no parameter named '{arg}'{suggestion}")
                }
                Self::TooManyArgs { name, expected, actual } => {
                    let p = plural_params(*expected, lang);
                    format!("Function '{name}' expects at most {p}, got {actual}")
                }
                Self::MissingArgument { name, arg, suggestion } => {
                    format!("Missing argument '{arg}' for function '{name}'{suggestion}")
                }
                Self::FuncArgTypeMismatch { name, arg, expected, actual } => {
                    format!("Argument '{arg}' of function '{name}' expects type '{expected}', got '{actual}'")
                }
                Self::FuncPositionalArgTypeMismatch { name, idx, expected, actual } => {
                    format!("Argument {idx} of function '{name}' expects type '{expected}', got '{actual}'")
                }
                Self::InvalidEnumValue { value, expected } => {
                    format!("Invalid enum value '{value}'. Expected one of: {expected}")
                }
                Self::EnumIndexOutOfBounds { idx, max } => {
                    format!("Enum index {idx} out of bounds. Max index is {max}")
                }
                Self::InvalidBoolEnum { expected } => {
                    format!("Invalid boolean value for enum. Expected one of: {expected}")
                }
                Self::UnknownGameValue { name } => format!("Unknown game value: {name}"),
                Self::InvalidSelector { selector, object } => {
                    format!("Unknown or invalid selector: '{selector}' for object '{object}'")
                }
                Self::ActionArgTypeMismatch { name, arg, expected, actual } => {
                    format!("Argument '{arg}' of action '{name}' expects type '{expected}', got '{actual}'")
                }
                Self::InfinitePropertyRecursion { property } => {
                    format!("Infinite recursion detected: accessing property '{property}' on 'self' inside its own getter or setter")
                }
                Self::CyclicInheritance { class } => {
                    format!("Cyclic inheritance detected for class '{class}'")
                }
                Self::CyclicTypeInference => {
                    "Cyclic type inference detected (likely caused by a type mismatch or invalid cast)".to_owned()
                }
                Self::UnknownType(s) => format!("Unknown type: {s}"),
                Self::UndeclaredVariable { name, suggestion } => {
                    format!("Use of undeclared variable: '{name}'{suggestion}")
                }
                Self::NotIterable { ty } => {
                    format!("Type '{ty}' is not iterable in 'for' loop (expected array or map)")
                }
                Self::MatchArmTypeMismatch { expected, actual } => {
                    format!("Match arms have incompatible types: arm has type '{actual}', but previous arm produced '{expected}'")
                }
                Self::TernaryBranchTypeMismatch { then_ty, else_ty } => {
                    format!("Ternary expression branches have incompatible types: 'then' branch is '{then_ty}', but 'else' branch is '{else_ty}'")
                }
                Self::OperationOnAny { op } => {
                    format!("Cannot perform binary operation '{op}' on value of type 'any'")
                }
                Self::PropertyAccessOnAny { property } => {
                    format!("Cannot access property '{property}' on value of type 'any'")
                }
                Self::MethodCallOnAny { method } => {
                    format!("Cannot call method '{method}' on value of type 'any'")
                }
                Self::VoidReturnValueUsed { name } => format!("Function '{name}' does not return a value"),
                Self::ProcessReturnValueUsed { name } => format!("Process '{name}' does not return a value"),
                Self::UnresolvedTypeInference => {
                    "Could not infer type for this expression. Please add explicit type annotations.".to_owned()
                }
                Self::RedundantCast { ty } => format!("Redundant cast: value is already of type '{ty}'"),
                Self::MissingInterfaceMethod { class, interface, method } => {
                    format!("Class '{class}' does not implement method '{method}' from interface '{interface}'")
                }
                Self::InterfaceMethodSignatureMismatch { class, interface, method, details } => {
                    format!("Method '{method}' in class '{class}' has mismatched signature with interface '{interface}': {details}")
                }
                Self::CyclicInterfaceInheritance { interface } => {
                    format!("Cyclic interface inheritance detected for interface '{interface}'")
                }
                Self::DeprecatedElif => {
                    "use of 'elif' is deprecated, use 'match' instead".to_owned()
                }
                Self::EventNotCancellable { event } => {
                    format!("Event '{event}' is not cancellable")
                }
            },
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SemanticError {
    Single {
        kind: Box<SemanticErrorKind>,
        span: String,
        rendered: Option<String>,
    },
    Multiple {
        count: usize,
        details: String,
        rendered: Option<String>,
    },
}

impl std::error::Error for SemanticError {}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format_localized(current_lang()))
    }
}

impl SemanticError {
    #[must_use]
    pub fn format_localized(&self, lang: Lang) -> String {
        match self {
            Self::Single {
                kind,
                span,
                rendered,
            } => rendered.as_deref().map_or_else(
                || match lang {
                    Lang::Ru => format!(
                        "Семантическая ошибка в {span}: {}",
                        kind.format_localized(lang)
                    ),
                    Lang::En => {
                        format!("Semantic error at {span}: {}", kind.format_localized(lang))
                    }
                },
                str::to_owned,
            ),
            Self::Multiple {
                count,
                details,
                rendered,
            } => rendered.as_deref().map_or_else(
                || {
                    let header = plural_errors(*count, lang);
                    format!("{header}:\n{details}")
                },
                str::to_owned,
            ),
        }
    }
}

pub struct Analyzer<'a> {
    ast: &'a Ast,
    source: &'a str,
    ir_ctx: &'a IrCtx,
    scopes: Vec<HashMap<String, Symbol>>,
    errors: Vec<(SemanticErrorKind, Span)>,
    warnings: Vec<(SemanticErrorKind, Span)>,
    in_function: bool,
    in_process: bool,
    function_return_type: Option<Type>,
    loop_depth: usize,
    loop_labels: Vec<Option<StrId>>,
    declared_top_level: HashSet<String>,
    statement_context: bool,
    pub expr_types: HashMap<ExprId, Type>,
    unifier: Unifier,
    generic_scopes: Vec<HashMap<StrId, Type>>,
    getter_setter_stack: Vec<String>,
    default_scope: VarScope,
    edition: u16,
    in_lambda: bool,
    pub current_event: Option<String>,
    /// Inference vars of unannotated parameters, which may stay unconstrained: `__set_attribute__`
    /// is called implicitly (`obj.x = 5`), so a property nobody reads has no type to infer from.
    /// Leaving one open is not an inference failure.
    open_param_vars: HashSet<usize>,
    /// `std` root, resolved once in [`Analyzer::new`]: `std_lib_root` hits the filesystem.
    std_root: PathBuf,
}

impl<'a> Analyzer<'a> {
    #[instrument(skip(ast, source, ir_ctx), level = "debug")]
    #[must_use]
    pub fn new(ast: &'a Ast, source: &'a str, ir_ctx: &'a IrCtx, edition: u16) -> Self {
        Self {
            ast,
            source,
            ir_ctx,
            scopes: vec![HashMap::new()],
            errors: Vec::new(),
            warnings: Vec::new(),
            in_function: false,
            in_process: false,
            function_return_type: None,
            loop_depth: 0,
            loop_labels: Vec::new(),
            declared_top_level: HashSet::new(),
            statement_context: false,
            expr_types: HashMap::new(),
            unifier: Unifier::new(),
            generic_scopes: vec![HashMap::new()],
            getter_setter_stack: Vec::new(),
            default_scope: if edition < 2026 {
                VarScope::Local
            } else {
                VarScope::Line
            },
            edition,
            in_lambda: false,
            current_event: None,
            open_param_vars: HashSet::new(),
            std_root: crate::ast::import::std_lib_root(),
        }
    }

    #[instrument(skip(self, kind), level = "debug")]
    fn error(&mut self, kind: SemanticErrorKind, span: Span) {
        trace!(error = %kind, span = ?span, "Semantic error recorded");
        self.errors.push((kind, span));
    }

    #[instrument(skip(self, kind), level = "debug")]
    fn warning(&mut self, kind: SemanticErrorKind, span: Span) {
        trace!(warning = %kind, span = ?span, "Semantic warning recorded");
        self.warnings.push((kind, span));
    }

    fn with_generic_scope<F, R>(&mut self, generics: &[StrId], f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut scope = HashMap::new();
        for &g in generics {
            scope.insert(g, Type::Param(g));
        }
        self.generic_scopes.push(scope);
        let res = f(self);
        self.generic_scopes.pop();
        res
    }

    /// File the span belongs to, with its start offset in the concatenated sources.
    ///
    /// Nearest segment rather than containing one: a span may stick out slightly, as a node
    /// inserted from a macro does.
    fn span_file(&self, span: &Span) -> Option<(&Path, usize)> {
        self.ast
            .file_offsets
            .iter()
            .min_by_key(|(_, start, end)| {
                if span.start >= *start && span.start < *end {
                    0
                } else if span.start < *start {
                    *start - span.start
                } else {
                    span.start - *end
                }
            })
            .map(|(path, start, _)| (path.as_path(), *start))
    }

    /// Whether the span lies in a standard library file.
    ///
    /// `std` implements the operators with exactly the raw actions [`Analyzer::lint_raw_action`]
    /// warns about (`__add__` is `variable::add`), so it is excluded by path.
    fn is_std_span(&self, span: &Span) -> bool {
        self.span_file(span)
            .is_some_and(|(path, _)| path.starts_with(&self.std_root))
    }

    #[instrument(skip(self), level = "trace")]
    fn format_span(&self, span: &Span) -> String {
        if self.ast.file_offsets.is_empty() {
            return format!("{}:{}", span.start, span.end);
        }

        let Some((path, start_offset)) = self.span_file(span) else {
            return format!("{}:{}", span.start, span.end);
        };

        let active_path = Some(path);
        let active_source = self
            .ast
            .sources
            .get(path)
            .map(|s| s.as_str())
            .unwrap_or(self.source);

        let local_start = span
            .start
            .saturating_sub(start_offset)
            .min(active_source.len());

        // Fast path: precomputed line starts index for O(log n) lookup.
        let indexed_line_col = self.ast.line_indexes.get(path).and_then(|index| {
            let pos = TextSize::from(u32::try_from(local_start).ok()?);
            index.try_line_col(pos)
        });
        // Fallback: byte arithmetic when index is unavailable.
        let byte_line_col = || {
            let head = &active_source[..local_start];
            let line = head.matches('\n').count() + 1;
            let col = local_start - head.rfind('\n').map_or(0, |p| p + 1) + 1;
            (line, col)
        };
        let (sl, sc) = indexed_line_col.map_or_else(byte_line_col, |lc| {
            (lc.line as usize + 1, lc.col as usize + 1)
        });

        let prefix = active_path
            .map(|path| {
                let rel = std::env::current_dir()
                    .ok()
                    .and_then(|cwd| path.strip_prefix(&cwd).ok())
                    .unwrap_or(path);
                format!("{}:", rel.display())
            })
            .unwrap_or_default();
        format!("{prefix}{sl}:{sc}")
    }

    /// # Errors
    ///
    /// Returns the semantic diagnostics found while analyzing the syntax tree.
    #[instrument(skip(self), level = "info")]
    #[expect(
        clippy::too_many_lines,
        reason = "Full semantic analysis pipeline with diagnostic rendering"
    )]
    pub fn analyze(mut self) -> Result<HashMap<ExprId, Type>, SemanticError> {
        info!("Semantic analysis started.");
        self.predeclare_top_level();
        info!("Top-level declarations registered.");

        for stmt in &self.ast.statements {
            self.analyze_stmt(stmt);
        }

        let mut unresolved_exprs = Vec::new();
        for (&eid, ty) in &mut self.expr_types {
            let resolved = self.unifier.find(ty);
            if let Type::InferVar(id) = resolved {
                if !self.open_param_vars.contains(&id) {
                    unresolved_exprs.push(eid);
                }
                *ty = Type::Unknown;
            } else {
                *ty = resolved;
            }
        }

        if self.edition >= 2026 {
            for eid in unresolved_exprs {
                let span = self.ast.exprs[eid].span();
                self.error(SemanticErrorKind::UnresolvedTypeInference, span);
            }
        } else if !unresolved_exprs.is_empty() {
            warn!(
                count = unresolved_exprs.len(),
                "Unresolved type inference defaulted to Unknown (legacy edition)"
            );
        }

        if self.unifier.had_cycle {
            self.error(SemanticErrorKind::CyclicTypeInference, 0..0);
        }

        if self.errors.is_empty() {
            info!("Semantic analysis completed successfully.");
            if !self.warnings.is_empty() {
                let lang = current_lang();
                let diags: Vec<_> = self
                    .warnings
                    .iter()
                    .map(|(k, s)| crate::diagnostic::semantic_to_diagnostic(k, s, self.ast, lang))
                    .collect();
                let renderer = crate::diagnostic::DiagnosticRenderer::default().with_lang(lang);
                let rendered = renderer.render_ast_diagnostics(&diags, self.ast, lang);
                #[expect(clippy::print_stderr, reason = "compiler warning output to stderr")]
                {
                    eprintln!("{rendered}");
                }
            }
            Ok(self.expr_types)
        } else if self.errors.len() == 1 {
            let (kind, span) = self.errors.remove(0);
            let lang = current_lang();
            let span_str = self.format_span(&span);
            let mut diags: Vec<_> = self
                .warnings
                .iter()
                .map(|(k, s)| crate::diagnostic::semantic_to_diagnostic(k, s, self.ast, lang))
                .collect();
            diags.push(crate::diagnostic::semantic_to_diagnostic(
                &kind, &span, self.ast, lang,
            ));
            let renderer = crate::diagnostic::DiagnosticRenderer::default().with_lang(lang);
            let rendered = renderer.render_ast_diagnostics(&diags, self.ast, lang);
            Err(SemanticError::Single {
                kind: Box::new(kind),
                span: span_str,
                rendered: Some(rendered),
            })
        } else {
            let count = self.errors.len();
            let lang = current_lang();
            let details = self
                .errors
                .iter()
                .map(|(k, s)| match lang {
                    Lang::Ru => format!(
                        "  - в {}: {}",
                        self.format_span(s),
                        k.format_localized(lang)
                    ),
                    Lang::En => format!(
                        "  - at {}: {}",
                        self.format_span(s),
                        k.format_localized(lang)
                    ),
                })
                .collect::<Vec<_>>()
                .join("\n");

            let mut diags: Vec<_> = self
                .warnings
                .iter()
                .map(|(k, s)| crate::diagnostic::semantic_to_diagnostic(k, s, self.ast, lang))
                .collect();
            diags.extend(
                self.errors
                    .iter()
                    .map(|(k, s)| crate::diagnostic::semantic_to_diagnostic(k, s, self.ast, lang)),
            );
            let renderer = crate::diagnostic::DiagnosticRenderer::default().with_lang(lang);
            let rendered = renderer.render_ast_diagnostics(&diags, self.ast, lang);

            Err(SemanticError::Multiple {
                count,
                details,
                rendered: Some(rendered),
            })
        }
    }

    /// Performs semantic analysis collecting both types and diagnostics without early aborting.
    #[instrument(skip(self), level = "info")]
    pub fn analyze_for_diagnostics(
        mut self,
    ) -> (HashMap<ExprId, Type>, Vec<(SemanticErrorKind, Span)>) {
        info!("Semantic analysis for diagnostics started.");
        self.predeclare_top_level();
        for stmt in &self.ast.statements {
            self.analyze_stmt(stmt);
        }

        let mut unresolved_exprs = Vec::new();
        for (&eid, ty) in &mut self.expr_types {
            let resolved = self.unifier.find(ty);
            if let Type::InferVar(id) = resolved {
                if !self.open_param_vars.contains(&id) {
                    unresolved_exprs.push(eid);
                }
                *ty = Type::Unknown;
            } else {
                *ty = resolved;
            }
        }

        if self.edition >= 2026 {
            for eid in unresolved_exprs {
                let span = self.ast.exprs[eid].span();
                self.error(SemanticErrorKind::UnresolvedTypeInference, span);
            }
        }

        if self.unifier.had_cycle {
            self.error(SemanticErrorKind::CyclicTypeInference, 0..0);
        }

        let mut all_diags = self.warnings;
        all_diags.extend(self.errors);
        (self.expr_types, all_diags)
    }
}

/// # Errors
///
/// Returns the semantic diagnostics found while analyzing `ast`.
#[instrument(skip(ast, source, ir_ctx), level = "info")]
pub fn analyze(
    ast: &Ast,
    source: &str,
    ir_ctx: &IrCtx,
    edition: u16,
) -> Result<HashMap<ExprId, Type>, SemanticError> {
    Analyzer::new(ast, source, ir_ctx, edition).analyze()
}

/// Runs semantic analysis returning both inferred types and collected diagnostics.
#[must_use]
#[instrument(skip(ast, source, ir_ctx), level = "info")]
pub fn analyze_for_diagnostics(
    ast: &Ast,
    source: &str,
    ir_ctx: &IrCtx,
    edition: u16,
) -> (HashMap<ExprId, Type>, Vec<(SemanticErrorKind, Span)>) {
    Analyzer::new(ast, source, ir_ctx, edition).analyze_for_diagnostics()
}
