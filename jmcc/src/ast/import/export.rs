//! Export filtering: declarations surviving import and their exported names.

use std::collections::{HashMap, HashSet};

use lasso::Rodeo;
use tracing::{Level, debug, instrument, span, trace};

use super::*;

impl ImportResolver {
    #[instrument(skip(ast), level = "trace")]
    pub(super) fn get_existing_names(ast: &Ast) -> HashSet<String> {
        let names: HashSet<String> = ast
            .statements
            .iter()
            .filter_map(|s| Self::single_name(s, &ast.strings))
            .collect();
        trace!(count = names.len(), "Existing names collected");
        names
    }

    /// Returns the declaration name entered into the symbol table under a single key.
    pub(super) fn single_name(statement: &Statement, strings: &Rodeo) -> Option<String> {
        match statement {
            Statement::Function(function) => Some(strings.resolve(&function.name).to_owned()),
            Statement::Process(process) => Some(strings.resolve(&process.name).to_owned()),
            Statement::Class(class) => Some(strings.resolve(&class.name).to_owned()),
            Statement::Interface(interface) => Some(strings.resolve(&interface.name).to_owned()),
            Statement::TypeAlias(alias) => Some(strings.resolve(&alias.name).to_owned()),
            Statement::Enum(enumeration) => Some(strings.resolve(&enumeration.name).to_owned()),
            _ => None,
        }
    }

    const fn is_exported(stmt: &Statement) -> bool {
        match stmt {
            Statement::Function(f) => f.is_exported,
            Statement::Process(p) => p.is_exported,
            Statement::VarDecl(v) => v.is_exported,
            Statement::Class(c) => c.is_exported,
            Statement::Interface(i) => i.is_exported,
            Statement::TypeAlias(ta) => ta.is_exported,
            Statement::Enum(e) => e.is_exported,
            _ => false,
        }
    }

    #[instrument(skip(self, dst, src), fields(kind = ?kind, src_module = %_src_module_name), level = "debug")]
    pub(super) fn filter_imported_statements(
        &self,
        dst: &mut Ast,
        src: Ast,
        kind: &ImportKind,
        _src_module_name: &str,
        import_map: &mut HashMap<String, String>,
    ) -> Vec<Statement> {
        let _span = span!(Level::DEBUG, "filter_imported_statements").entered();
        let expr_map = self.transfer_exprs(dst, &src);
        let existing = Self::get_existing_names(dst);

        match kind {
            ImportKind::SideEffect => {
                debug!("Filtering import: SideEffect");
                let result = self.filter_side_effect(dst, &src, &expr_map, &existing, import_map);
                debug!(kept = result.len(), "SideEffect import filtered");
                result
            }
            ImportKind::Default(_) | ImportKind::Namespace(_) => {
                debug!("Filtering import: Default/Namespace");
                let result: Vec<Statement> = src
                    .statements
                    .iter()
                    .filter(|s| Self::is_exported(s))
                    .filter(|s| {
                        Self::single_name(s, &src.strings)
                            .is_none_or(|name| !existing.contains(&name))
                    })
                    .map(|s| self.remap_stmt(s, &src, dst, &expr_map))
                    .collect();
                debug!(kept = result.len(), "Default/Namespace import filtered");
                result
            }
            ImportKind::Named(items) => {
                debug!(items = items.len(), "Filtering import: Named");
                let result = self.filter_named(dst, &src, &expr_map, &existing, items, import_map);
                debug!(kept = result.len(), "Named import filtered");
                result
            }
        }
    }

    fn filter_side_effect(
        &self,
        dst: &mut Ast,
        src: &Ast,
        expr_map: &HashMap<ExprId, ExprId>,
        existing: &HashSet<String>,
        import_map: &mut HashMap<String, String>,
    ) -> Vec<Statement> {
        let mut result = Vec::new();
        for statement in &src.statements {
            if matches!(statement, Statement::Import(_))
                || matches!(statement, Statement::Class(class) if !class.is_exported)
                || matches!(statement, Statement::Interface(interface) if !interface.is_exported)
                || matches!(statement, Statement::TypeAlias(alias) if !alias.is_exported)
                || matches!(statement, Statement::Enum(enumeration) if !enumeration.is_exported)
            {
                continue;
            }

            // Non-exported `var` does not introduce names, but remains in the statements.
            let (names, is_function) = match statement {
                Statement::VarDecl(declaration) if !declaration.is_exported => (Vec::new(), false),
                _ => Self::importable_names(statement, src).unwrap_or_default(),
            };

            let mut duplicate = false;
            let canonical_target = names.first().cloned();
            for (i, name) in names.iter().enumerate() {
                if is_function && Self::is_exported(statement) {
                    let local = name.rsplit("::").next().unwrap_or(name).to_owned();
                    if let Some(target) = &canonical_target {
                        import_map.insert(local, target.clone());
                    }
                }
                if i == 0 && existing.contains(name) {
                    trace!(name = %name, "Skipping already-existing name");
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                result.push(self.remap_stmt(statement, src, dst, expr_map));
            }
        }
        result
    }

    fn filter_named(
        &self,
        dst: &mut Ast,
        src: &Ast,
        expr_map: &HashMap<ExprId, ExprId>,
        existing: &HashSet<String>,
        items: &[ImportItem],
        import_map: &mut HashMap<String, String>,
    ) -> Vec<Statement> {
        let mut result = Vec::new();
        for statement in &src.statements {
            let Some((names, is_function)) = Self::importable_names(statement, src) else {
                continue;
            };
            let mut matches = Vec::new();
            for (index, name) in names.iter().enumerate() {
                let local_name = if is_function {
                    name.rsplit("::").next().unwrap_or(name)
                } else {
                    name
                };
                if let Some(item) = items
                    .iter()
                    .find(|item| src.strings.resolve(&item.original) == local_name)
                {
                    matches.push((index, dst.strings.resolve(&item.local).to_owned()));
                }
            }
            if matches.is_empty() {
                continue;
            }

            let mut remapped = self.remap_stmt(statement, src, dst, expr_map);
            if is_function {
                for (_, local_name) in &matches {
                    import_map.insert(local_name.clone(), names[0].clone());
                }
                if !existing.contains(&names[0]) {
                    result.push(remapped);
                } else {
                    trace!(name = %names[0], "Named import already exists — skipping body");
                }
                continue;
            }

            if let Statement::VarDecl(declaration) = &mut remapped {
                for (index, _) in &matches {
                    if let Some(item) = items
                        .iter()
                        .find(|item| src.strings.resolve(&item.original) == names[*index])
                    {
                        declaration.names[*index] = TextValue {
                            parts: vec![TextPart::Literal(item.local)],
                            parsing: TextParsing::Legacy,
                            span: declaration.span.clone(),
                        };
                    }
                }
            }
            result.push(remapped);
        }
        result
    }

    /// Returns the names under which a declaration is imported, and whether it is a function.
    fn importable_names(statement: &Statement, src: &Ast) -> Option<(Vec<String>, bool)> {
        if let Statement::VarDecl(declaration) = statement {
            let names = declaration
                .names
                .iter()
                .map(|name| text_value_to_string(src, name))
                .collect();
            return Some((names, false));
        }
        let name = Self::single_name(statement, &src.strings)?;
        let is_function = matches!(statement, Statement::Function(_) | Statement::Process(_));
        let mut names = vec![name];
        match statement {
            Statement::Function(f) => {
                for a in &f.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            Statement::Process(p) => {
                for a in &p.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            Statement::Class(c) => {
                for a in &c.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            Statement::Interface(i) => {
                for a in &i.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            Statement::TypeAlias(t) => {
                for a in &t.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            Statement::Enum(e) => {
                for a in &e.aliases {
                    names.push(src.strings.resolve(a).to_owned());
                }
            }
            _ => {}
        }
        Some((names, is_function))
    }
}
