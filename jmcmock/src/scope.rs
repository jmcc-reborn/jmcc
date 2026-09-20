//! `JustMC` variable scopes and their lifetimes.
//!
//! The four scopes differ not so much in visibility as in lifetime:
//!
//! | Scope | Lives for | Stored in |
//! |---|---|---|
//! | `line` | one call frame | [`Stream::line`](crate::Stream), one per frame |
//! | `local` | one process or event, with all nested calls | [`Shared`], one per process |
//! | `game` | the whole run of the code | [`Runtime`](crate::Runtime) |
//! | `save` | like `game`, but survives a restart | [`Runtime`](crate::Runtime) |
//!
//! `JustMC` creates line variables at the start of a line rather than at the
//! first assignment: the compiler freely reads a variable that was written on
//! only one of the branches (that is how the temporary `__ct` works), and that
//! works precisely because the variable is already there by the start of the
//! line — empty. The mock repeats this: [`declare_line_vars`] walks the frame's
//! body and declares every name that occurs anywhere in it with `scope: line`.
//!
//! Hence three distinguishable states of a variable, and they mean different
//! things:
//!
//! * **not in the store** — the variable does not exist, and reading it is the
//!   error [`RuntimeError::UndefinedVariable`];
//! * **present, value [`None`]** — the variable exists but is empty; that is
//!   the normal state of a line variable that was never written, and `JustMC`
//!   substitutes `0` or an empty text for it;
//! * **present, value `Some`** — an ordinary read.
//!
//! Returning a value from a function rests on a shared `local`: the compiler
//! writes `ret` as a local variable, the callee fills it in, the caller reads
//! it right after the call (see `Mir::lower_func_call` in `jmcc`). That is why
//! `local` has to be one and the same store for a whole process or event.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use jmcdata::module::{Op, Value, VariableScope};

use crate::error::{Result, RuntimeError};
use crate::value::{Rt, Vars, is_unset};

/// The variable store of one scope, shared between frames.
///
/// Cloning gives another reference to the same store rather than a copy: that
/// is exactly how one process's `local` ends up shared by all its frames.
///
/// Besides its own variables a store can hold **links**: names that are not
/// stored here but stand for a name in another store. That is how a
/// `variable`-typed parameter works — the callee's `self` is not a copy of the
/// caller's variable but the caller's variable itself, so `self.layers = layers`
/// inside a method is visible to whoever called it. `ref` in `.jc` and a
/// method's receiver both ask for that, and the compiler marks them with
/// `value_type: "variable"` in the module.
#[derive(Debug, Clone)]
pub struct Shared<'a> {
    vars: Rc<RefCell<Vars<'a>>>,
    links: Rc<RefCell<HashMap<String, Link<'a>>>>,
}

/// A name in one store that stands for a name in another.
///
/// Links always point outwards — from a callee's `line` to its caller's. A
/// frame gets a fresh `line` on every call, so a chain of links ends at a store
/// which belongs to a frame still on the stack and cannot come back around.
#[derive(Debug, Clone)]
struct Link<'a> {
    store: Shared<'a>,
    name: String,
}

impl<'a> Shared<'a> {
    /// An empty store.
    #[must_use]
    #[tracing::instrument(level = "trace")]
    pub fn new() -> Self {
        Self {
            vars: Rc::new(RefCell::new(Vars::new())),
            links: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// A store with contents already in place.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(vars))]
    pub fn with_vars(vars: Vars<'a>) -> Self {
        Self {
            vars: Rc::new(RefCell::new(vars)),
            links: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Binds `name` here to `name` in `store`.
    ///
    /// Every read, write, declaration and removal of `name` afterwards goes to
    /// `store`. See [`Link`].
    #[tracing::instrument(level = "trace", skip(self, store), fields(name = %name, target = %target))]
    pub fn link(&self, name: &str, store: Self, target: &str) {
        self.links.borrow_mut().insert(
            name.to_owned(),
            Link {
                store,
                name: target.to_owned(),
            },
        );
    }

    /// Whether `name` stands for a variable in another store.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn is_link(&self, name: &str) -> bool {
        self.links.borrow().contains_key(name)
    }

    /// The store and name a link points at; `None` for an ordinary variable.
    ///
    /// The link is cloned out, so the borrow of the link table ends here: the
    /// caller then reads or writes the other store without holding this one.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    fn link_to(&self, name: &str) -> Option<Link<'a>> {
        self.links.borrow().get(name).cloned()
    }

    /// How many variables the store has, links included.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn len(&self) -> usize {
        self.vars.borrow().len() + self.links.borrow().len()
    }

    /// Whether the store is empty.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn is_empty(&self) -> bool {
        self.vars.borrow().is_empty() && self.links.borrow().is_empty()
    }

    /// Whether the variable exists. An empty variable exists.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn contains(&self, name: &str) -> bool {
        self.link_to(name).map_or_else(
            || self.vars.borrow().contains_key(name),
            |link| link.store.contains(&link.name),
        )
    }

    /// Creates the variable if it is not there yet, and leaves the value alone
    /// if it is.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn declare(&self, name: &str) {
        match self.link_to(name) {
            Some(link) => link.store.declare(&link.name),
            None => {
                self.vars
                    .borrow_mut()
                    .entry(name.to_owned())
                    .or_insert(None);
            }
        }
    }

    /// Reads a variable without treating its absence as an error: `None` means
    /// no such variable, `Some(None)` an empty variable, `Some(Some(..))` a
    /// value.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn peek(&self, name: &str) -> Option<Rt<'a>> {
        match self.link_to(name) {
            Some(link) => link.store.peek(&link.name),
            None => self.vars.borrow().get(name).cloned(),
        }
    }

    /// Reads a variable, treating its absence as an error.
    ///
    /// Returns `None` if the variable exists but is empty — that is not an
    /// error: an empty value is legitimate in itself, and whether a concrete
    /// type is needed is for the caller to decide.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UndefinedVariable`] if there is no such variable.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name, scope = ?scope))]
    pub fn read(&self, name: &str, scope: VariableScope) -> Result<Rt<'a>> {
        self.peek(name)
            .ok_or_else(|| RuntimeError::UndefinedVariable {
                name: name.to_owned(),
                scope,
            })
    }

    /// Reads a variable whose absence is normal.
    ///
    /// `JustMC` has no "undefined variable": a variable nobody wrote yet reads
    /// as empty, whatever its scope, and a program relies on that. The pillars in
    /// `tests/pvp` wait on a per-player `game` flag until another handler sets
    /// it, and a `local` written on one branch alone is read on the other.
    ///
    /// `line` is the exception, and deliberately so: the mock declares every
    /// line variable the frame's body mentions before it starts (see
    /// [`line_var_names`](crate::scope::line_var_names)), so a read that misses
    /// means the module and the mock disagree about the frame — a bug worth
    /// stopping on rather than hiding under an empty value.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UndefinedVariable`] for a missing `line`
    /// variable.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name, scope = ?scope))]
    pub fn read_or_empty(&self, name: &str, scope: VariableScope) -> Result<Rt<'a>> {
        match self.read(name, scope) {
            Err(RuntimeError::UndefinedVariable { .. }) if scope != VariableScope::Line => Ok(None),
            other => other,
        }
    }

    /// Writes a value, creating the variable if necessary.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name, value = ?value))]
    pub fn set(&self, name: &str, value: Rt<'a>) {
        match self.link_to(name) {
            Some(link) => link.store.set(&link.name, value),
            None => {
                self.vars.borrow_mut().insert(name.to_owned(), value);
            }
        }
    }

    /// Removes a variable. A missing variable is not an error.
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn remove(&self, name: &str) {
        match self.link_to(name) {
            Some(link) => link.store.remove(&link.name),
            None => {
                self.vars.borrow_mut().remove(name);
            }
        }
    }

    /// Empties the store, links included.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn clear(&self) {
        self.vars.borrow_mut().clear();
        self.links.borrow_mut().clear();
    }

    /// The variable names, in ascending order. A linked name is listed under
    /// the name it is known by here.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn names(&self) -> Vec<String> {
        let mut names: BTreeSet<String> = self.vars.borrow().keys().cloned().collect();
        names.extend(self.links.borrow().keys().cloned());
        names.into_iter().collect()
    }

    /// A copy of the contents. Needed where the store has to be walked in full
    /// without holding a borrow: holding one is not an option, because the
    /// elements are the very values the running code writes.
    ///
    /// Linked names are resolved: a copy is a copy of the value, not of the
    /// reference — that is what makes it a copy.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn snapshot(&self) -> Vars<'a> {
        let mut vars = self.vars.borrow().clone();
        for name in self.links.borrow().keys() {
            vars.insert(name.clone(), self.peek(name).flatten());
        }
        vars
    }

    /// Replaces the contents wholesale, dropping every link: the links belong to
    /// the frame that was called, not to the contents.
    #[tracing::instrument(level = "trace", skip(self, vars))]
    pub fn restore(&self, vars: Vars<'a>) {
        *self.vars.borrow_mut() = vars;
        self.links.borrow_mut().clear();
    }
}

impl Default for Shared<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The names of the line variables occurring in a frame's body, alphabetically.
///
/// The list is computed once when the module is loaded and kept next to the
/// handler: the frame's body never changes, while it is called many times.
#[must_use]
#[tracing::instrument(level = "trace", skip(ops))]
pub fn line_var_names(ops: &[Op<'_>]) -> Vec<String> {
    scoped_var_names(ops, VariableScope::Line)
}

/// Returns variable names of the requested persistent scope occurring in a block.
#[must_use]
#[tracing::instrument(level = "trace", skip(ops), fields(scope = ?scope))]
pub fn scoped_var_names(ops: &[Op<'_>], scope: VariableScope) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_scoped_names(ops, scope, &mut names);
    names.into_iter().collect()
}

/// Declares in the store every name occurring in the frame's body with
/// `scope: line`.
///
/// This reproduces the `JustMC` rule: line variables exist from the start of
/// the line, so reading a variable written on only one of the branches gives an
/// empty value rather than an error.
#[tracing::instrument(level = "trace", skip(line, ops))]
pub fn declare_line_vars<'a>(line: &Shared<'a>, ops: &[Op<'a>]) {
    for name in line_var_names(ops) {
        line.declare(&name);
    }
}

/// Collects the line-variable names mentioned in the operations, nested blocks
/// included.
#[tracing::instrument(level = "trace", skip(ops, names), fields(scope = ?scope))]
fn collect_scoped_names(ops: &[Op<'_>], scope: VariableScope, names: &mut BTreeSet<String>) {
    for op in ops {
        for value in op.values.values() {
            collect_scoped_names_in(value, scope, names);
        }
        if let Some(nested) = &op.operations {
            collect_scoped_names(nested, scope, names);
        }
    }
}

/// Collects names of one scope inside a single value, nested lists and maps
/// included.
#[tracing::instrument(level = "trace", skip(names), fields(value = ?value, wanted = ?wanted))]
fn collect_scoped_names_in(value: &Value<'_>, wanted: VariableScope, names: &mut BTreeSet<String>) {
    match value {
        Value::Variable { variable, scope } if *scope == wanted => {
            names.insert(variable.to_string());
        }
        Value::Array { values } => {
            for item in values.iter().flatten() {
                collect_scoped_names_in(item, wanted, names);
            }
        }
        Value::Map { values } => {
            for item in values.values() {
                collect_scoped_names_in(item, wanted, names);
            }
        }
        _ => {}
    }
}

/// Checks that a value is not empty where emptiness is not allowed.
///
/// # Errors
///
/// Returns [`RuntimeError::UnsetVariable`] if the value is empty.
#[tracing::instrument(level = "trace", fields(value = ?value, name = %name, scope = ?scope))]
pub fn require_set<'a>(value: Rt<'a>, name: &str, scope: VariableScope) -> Result<Value<'a>> {
    if is_unset(&value) {
        return Err(RuntimeError::UnsetVariable {
            name: name.to_owned(),
            scope,
        });
    }
    // `is_unset` has ruled out `None` and `Value::Error`; the rest is a value.
    value.ok_or_else(|| RuntimeError::UnsetVariable {
        name: name.to_owned(),
        scope,
    })
}
