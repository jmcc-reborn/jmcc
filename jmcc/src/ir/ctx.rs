use crate::ast::semantic::{DefId, Type};
use crate::ast::*;
use egg::Symbol;
use std::collections::HashMap;
use std::rc::Rc;
use tracing::{debug, info, instrument, trace, warn};

/// Finds the first top-level `<` in a type expression.
#[must_use]
pub fn find_top_level_lt(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' if depth == 0 => return Some(i),
            '<' => depth += 1,
            '>' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// An entity with a qualified name and `DefId` (common to classes and enums).
trait Resolvable {
    fn qualified_name(&self) -> &str;
    fn def_id(&self) -> DefId;
}

impl Resolvable for ClassInfo {
    fn qualified_name(&self) -> &str {
        &self.name
    }

    fn def_id(&self) -> DefId {
        self.def_id
    }
}

impl Resolvable for EnumInfo {
    fn qualified_name(&self) -> &str {
        &self.name
    }

    fn def_id(&self) -> DefId {
        self.def_id
    }
}

/// Resolves an entity name to its `DefId`: exact match first, then suffix match on `::name`.
fn resolve_named<'a, T: Resolvable + 'a>(
    by_name: &HashMap<String, DefId>,
    by_def: impl IntoIterator<Item = &'a T>,
    name: &str,
) -> Option<DefId> {
    if let Some(id) = by_name.get(name) {
        return Some(*id);
    }
    let suffix = format!("::{name}");
    by_def
        .into_iter()
        .find(|item| {
            let qname = item.qualified_name();
            qname == name || qname.ends_with(&suffix) || name.ends_with(&format!("::{qname}"))
        })
        .map(Resolvable::def_id)
}

/// Splits generic arguments at top-level commas.
#[must_use]
pub fn split_type_args(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                result.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        result.push(last);
    }
    result
}

#[derive(Clone)]
pub struct ClassInfo {
    pub def_id: DefId,
    pub name: String,
    pub aliases: Vec<String>,
    pub methods: HashMap<String, Rc<FunctionDecl>>,
    pub processes: HashMap<String, Rc<ProcessDecl>>,
    pub getters: HashMap<String, Rc<FunctionDecl>>,
    pub setters: HashMap<String, Rc<FunctionDecl>>,
    pub fields: HashMap<String, (Type, usize)>,
    pub parent: Option<DefId>,
    pub parent_args: Vec<Type>,
    pub implements: Vec<(DefId, Vec<Type>)>,
    pub lang_item: bool,
    pub is_dict: bool,
    pub is_interface: bool,
    pub generics: Vec<StrId>,
}

#[derive(Clone)]
pub struct EnumInfo {
    pub def_id: DefId,
    pub name: String,
    pub aliases: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Clone)]
pub struct AliasInfo {
    pub generics: Vec<StrId>,
    pub target_ty: String,
}

pub struct IrCtx {
    pub classes_by_def: HashMap<DefId, ClassInfo>,
    pub classes_by_name: HashMap<String, DefId>,
    pub enums_by_def: HashMap<DefId, EnumInfo>,
    pub enums_by_name: HashMap<String, DefId>,
    pub inline_funcs: HashMap<Symbol, Rc<FunctionDecl>>,
    pub var_types: HashMap<Symbol, Type>,
    pub lang_items: HashMap<String, DefId>,
    pub type_aliases: HashMap<String, AliasInfo>,
}

impl IrCtx {
    #[instrument(skip(ast), level = "debug")]
    #[expect(
        clippy::too_many_lines,
        reason = "Initializes IR context from AST declarations"
    )]
    #[must_use]
    pub fn new(ast: &Ast) -> Self {
        let _span = tracing::span!(tracing::Level::DEBUG, "ir_ctx_init").entered();
        let mut def_id_counter: DefId = 1;
        let mut classes_by_def = HashMap::new();
        let mut classes_by_name = HashMap::new();
        let mut enums_by_def = HashMap::new();
        let mut enums_by_name = HashMap::new();
        let mut type_aliases = HashMap::new();
        let inline_funcs = HashMap::new();
        let mut lang_items = HashMap::new();

        debug!("Collecting initial class and enum declarations");
        for stmt in &ast.statements {
            match stmt {
                Statement::Class(c) => {
                    let name = ast.strings.resolve(&c.name).to_owned();
                    let def_id = def_id_counter;
                    def_id_counter += 1;
                    let aliases: Vec<String> = c
                        .aliases
                        .iter()
                        .map(|a| ast.strings.resolve(a).to_owned())
                        .collect();
                    let info = ClassInfo {
                        def_id,
                        name: name.clone(),
                        aliases,
                        methods: HashMap::new(),
                        processes: HashMap::new(),
                        getters: HashMap::new(),
                        setters: HashMap::new(),
                        fields: HashMap::new(),
                        parent: None,
                        parent_args: Vec::new(),
                        implements: Vec::new(),
                        lang_item: c.lang_item,
                        generics: c.generics.clone(),
                        is_dict: c.is_dict,
                        is_interface: false,
                    };

                    if c.lang_item {
                        let short_name = name.rsplit("::").next().unwrap_or(&name).to_owned();
                        lang_items.insert(short_name, def_id);
                    }

                    trace!(class = %name, def_id, "Registered class");
                    classes_by_def.insert(def_id, info);
                    classes_by_name.insert(name, def_id);
                    for a in &c.aliases {
                        classes_by_name.insert(ast.strings.resolve(a).to_owned(), def_id);
                    }
                }
                Statement::Interface(i) => {
                    let name = ast.strings.resolve(&i.name).to_owned();
                    let def_id = def_id_counter;
                    def_id_counter += 1;
                    let aliases: Vec<String> = i
                        .aliases
                        .iter()
                        .map(|a| ast.strings.resolve(a).to_owned())
                        .collect();
                    let info = ClassInfo {
                        def_id,
                        name: name.clone(),
                        aliases,
                        methods: HashMap::new(),
                        processes: HashMap::new(),
                        getters: HashMap::new(),
                        setters: HashMap::new(),
                        fields: HashMap::new(),
                        parent: None,
                        parent_args: Vec::new(),
                        implements: Vec::new(),
                        lang_item: false,
                        generics: i.generics.clone(),
                        is_dict: false,
                        is_interface: true,
                    };
                    trace!(interface = %name, def_id, "Registered interface");
                    classes_by_def.insert(def_id, info);
                    classes_by_name.insert(name, def_id);
                    for a in &i.aliases {
                        classes_by_name.insert(ast.strings.resolve(a).to_owned(), def_id);
                    }
                }
                Statement::Enum(e) => {
                    let name = ast.strings.resolve(&e.name).to_owned();
                    let def_id = def_id_counter;
                    def_id_counter += 1;
                    let aliases: Vec<String> = e
                        .aliases
                        .iter()
                        .map(|a| ast.strings.resolve(a).to_owned())
                        .collect();
                    let info = EnumInfo {
                        def_id,
                        name: name.clone(),
                        aliases,
                        values: e
                            .values
                            .iter()
                            .map(|v| ast.strings.resolve(v).to_owned())
                            .collect(),
                    };
                    trace!(enum_name = %name, def_id, "Registered enum");
                    enums_by_def.insert(def_id, info);
                    enums_by_name.insert(name, def_id);
                    for a in &e.aliases {
                        enums_by_name.insert(ast.strings.resolve(a).to_owned(), def_id);
                    }
                }
                Statement::TypeAlias(ta) => {
                    let name = ast.strings.resolve(&ta.name).to_owned();
                    let target_ty = ast.strings.resolve(&ta.target_ty).to_owned();
                    type_aliases.insert(
                        name,
                        AliasInfo {
                            generics: ta.generics.clone(),
                            target_ty: target_ty.clone(),
                        },
                    );
                    for a in &ta.aliases {
                        type_aliases.insert(
                            ast.strings.resolve(a).to_owned(),
                            AliasInfo {
                                generics: ta.generics.clone(),
                                target_ty: target_ty.clone(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        let mut ctx = Self {
            classes_by_def,
            classes_by_name,
            enums_by_def,
            enums_by_name,
            inline_funcs,
            var_types: HashMap::new(),
            lang_items,
            type_aliases,
        };

        ctx.resolve_declarations(ast);

        info!("IrCtx initialized successfully");
        ctx
    }

    fn resolve_declarations(&mut self, ast: &Ast) {
        debug!("Resolving class bodies, methods, and fields");
        for statement in &ast.statements {
            if let Statement::Class(class) = statement {
                self.resolve_class(ast, class);
            } else if let Statement::Interface(interface) = statement {
                self.resolve_interface(ast, interface);
            } else if let Statement::Function(function) = statement
                && function.is_inline
            {
                let func_rc = Rc::new(function.clone());
                let symbol = Symbol::from(ast.strings.resolve(&function.name));
                self.inline_funcs.insert(symbol, Rc::clone(&func_rc));
                for a in &function.aliases {
                    let a_sym = Symbol::from(ast.strings.resolve(a));
                    self.inline_funcs.insert(a_sym, Rc::clone(&func_rc));
                }
            }
        }
    }

    fn resolve_class(&mut self, ast: &Ast, class: &ClassDecl) {
        let name = ast.strings.resolve(&class.name).to_owned();
        let Some(def_id) = self.classes_by_name.get(&name).copied() else {
            return;
        };
        let (parent, parent_args) = self.resolve_parent(ast, class);
        let implements = self.resolve_interface_list(ast, &class.implements, &class.generics);
        let mut methods = HashMap::new();
        let mut processes = HashMap::new();
        let mut getters = HashMap::new();
        let mut setters = HashMap::new();
        let mut fields = HashMap::new();

        for statement in &class.body {
            match statement {
                Statement::Function(function) => {
                    let full_name = ast.strings.resolve(&function.name).to_owned();
                    let short_name = full_name
                        .rsplit("::")
                        .next()
                        .unwrap_or(&full_name)
                        .to_owned();
                    let function_rc = Rc::new(function.clone());
                    insert_method_with_dunder_equivalents(
                        &mut methods,
                        &mut getters,
                        &mut setters,
                        Some(&mut self.inline_funcs),
                        Some(&full_name),
                        &short_name,
                        function,
                        &function_rc,
                    );
                    for a in &function.aliases {
                        let a_full = ast.strings.resolve(a).to_owned();
                        let a_short = a_full.rsplit("::").next().unwrap_or(&a_full).to_owned();
                        insert_method_with_dunder_equivalents(
                            &mut methods,
                            &mut getters,
                            &mut setters,
                            Some(&mut self.inline_funcs),
                            Some(&a_full),
                            &a_short,
                            function,
                            &function_rc,
                        );
                    }
                }
                Statement::Process(process) => {
                    let full_name = ast.strings.resolve(&process.name).to_owned();
                    let short_name = full_name
                        .rsplit("::")
                        .next()
                        .unwrap_or(&full_name)
                        .to_owned();
                    let process_rc = Rc::new(process.clone());
                    processes.insert(short_name, Rc::clone(&process_rc));
                    for a in &process.aliases {
                        let a_full = ast.strings.resolve(a).to_owned();
                        let a_short = a_full.rsplit("::").next().unwrap_or(&a_full).to_owned();
                        processes.insert(a_short, Rc::clone(&process_rc));
                    }
                }
                Statement::VarDecl(declaration) => {
                    for (index, field_name) in declaration.names.iter().enumerate() {
                        let name = text_value_to_string(ast, field_name);
                        let ty = declaration.tys.get(index).copied().flatten().map_or(
                            Type::Unknown,
                            |ty| {
                                self.type_from_str_with_generics(
                                    ast.strings.resolve(&ty),
                                    &class.generics,
                                    &ast.strings,
                                )
                            },
                        );
                        let index = fields.len();
                        fields.insert(name, (ty, index));
                    }
                }
                _ => {}
            }
        }

        if let Some(info) = self.classes_by_def.get_mut(&def_id) {
            info.parent = parent;
            info.parent_args = parent_args;
            info.implements = implements;
            info.methods = methods;
            info.processes = processes;
            info.getters = getters;
            info.setters = setters;
            info.fields = fields;
        }
    }

    fn resolve_interface(&mut self, ast: &Ast, interface: &InterfaceDecl) {
        let name = ast.strings.resolve(&interface.name).to_owned();
        let Some(def_id) = self.classes_by_name.get(&name).copied() else {
            return;
        };
        let implements = self.resolve_interface_list(ast, &interface.parents, &interface.generics);
        let mut methods = HashMap::new();
        let mut getters = HashMap::new();
        let mut setters = HashMap::new();

        for statement in &interface.body {
            if let Statement::Function(function) = statement {
                let full_name = ast.strings.resolve(&function.name).to_owned();
                let short_name = full_name
                    .rsplit("::")
                    .next()
                    .unwrap_or(&full_name)
                    .to_owned();
                let function_rc = Rc::new(function.clone());
                insert_method_with_dunder_equivalents(
                    &mut methods,
                    &mut getters,
                    &mut setters,
                    None,
                    Some(&full_name),
                    &short_name,
                    function,
                    &function_rc,
                );
                for a in &function.aliases {
                    let a_full = ast.strings.resolve(a).to_owned();
                    let a_short = a_full.rsplit("::").next().unwrap_or(&a_full).to_owned();
                    insert_method_with_dunder_equivalents(
                        &mut methods,
                        &mut getters,
                        &mut setters,
                        None,
                        Some(&a_full),
                        &a_short,
                        function,
                        &function_rc,
                    );
                }
            }
        }

        if let Some(info) = self.classes_by_def.get_mut(&def_id) {
            info.implements = implements;
            info.methods = methods;
            info.getters = getters;
            info.setters = setters;
        }
    }

    fn resolve_interface_list(
        &self,
        ast: &Ast,
        list: &[StrId],
        generics: &[StrId],
    ) -> Vec<(DefId, Vec<Type>)> {
        let mut result = Vec::new();
        for id in list {
            let name_str = ast.strings.resolve(id);
            match self.type_from_str_with_generics(name_str, generics, &ast.strings) {
                Type::Class(def_id, args) => result.push((def_id, args)),
                _ => {
                    if let Some(info) = self.get_class_by_name(name_str) {
                        result.push((info.def_id, Vec::new()));
                    }
                }
            }
        }
        result
    }

    fn resolve_parent(&self, ast: &Ast, class: &ClassDecl) -> (Option<DefId>, Vec<Type>) {
        let Some(parent_id) = class.parent else {
            return (None, Vec::new());
        };
        let parent_name = ast.strings.resolve(&parent_id);
        match self.type_from_str_with_generics(parent_name, &class.generics, &ast.strings) {
            Type::Class(id, args) => (Some(id), args),
            _ => (
                self.get_class_by_name(parent_name).map(|info| info.def_id),
                Vec::new(),
            ),
        }
    }

    #[must_use]
    pub fn substitute_type(ty: &Type, subst: &HashMap<StrId, Type>) -> Type {
        match ty {
            Type::Param(id) => subst.get(id).cloned().unwrap_or(Type::Unknown),
            Type::Class(id, args) => Type::Class(
                *id,
                args.iter()
                    .map(|a| Self::substitute_type(a, subst))
                    .collect(),
            ),
            _ => ty.clone(),
        }
    }

    #[must_use]
    pub fn get_class_name(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Class(def_id, _) => self.classes_by_def.get(def_id).map(|c| c.name.clone()),
            Type::Enum(def_id) => self.enums_by_def.get(def_id).map(|e| e.name.clone()),
            _ => None,
        }
    }

    #[must_use]
    pub fn get_class_by_name(&self, name: &str) -> Option<&ClassInfo> {
        let id = self.resolve_class_name(name)?;
        self.classes_by_def.get(&id)
    }

    #[must_use]
    pub fn get_class(&self, ty: &Type) -> Option<&ClassInfo> {
        if let Type::Class(def_id, _) = ty {
            self.classes_by_def.get(def_id)
        } else {
            None
        }
    }

    pub fn record_var_type(&mut self, name: Symbol, ty: Type) {
        self.var_types.insert(name, ty);
    }

    #[must_use]
    pub fn get_var_type(&self, name: &Symbol) -> Option<&Type> {
        self.var_types.get(name)
    }

    #[must_use]
    pub fn is_user_class(&self, name: &str) -> bool {
        if let Some(def_id) = self.classes_by_name.get(name)
            && let Some(info) = self.classes_by_def.get(def_id)
        {
            return !info.lang_item;
        }
        true
    }

    #[must_use]
    pub fn find_overload_method(
        &self,
        ty: &Type,
        method_name: &str,
    ) -> Option<(Rc<FunctionDecl>, String)> {
        let current_def = match ty {
            Type::Class(def_id, _) => *def_id,
            _ => return None,
        };
        let mut queue = std::collections::VecDeque::new();
        let mut visited = std::collections::HashSet::new();
        queue.push_back(current_def);
        visited.insert(current_def);

        while let Some(def_id) = queue.pop_front() {
            if let Some(class) = self.classes_by_def.get(&def_id) {
                if let Some(f) = class.methods.get(method_name) {
                    return Some((f.clone(), class.name.clone()));
                }
                if let Some(parent) = class.parent
                    && visited.insert(parent)
                {
                    queue.push_back(parent);
                }
                for (iface_id, _) in &class.implements {
                    if visited.insert(*iface_id) {
                        queue.push_back(*iface_id);
                    }
                }
            }
        }
        None
    }

    fn resolve_class_name(&self, name: &str) -> Option<DefId> {
        resolve_named(&self.classes_by_name, self.classes_by_def.values(), name)
    }

    fn resolve_enum_name(&self, name: &str) -> Option<DefId> {
        resolve_named(&self.enums_by_name, self.enums_by_def.values(), name)
    }

    #[must_use]
    pub fn get_type_alias(&self, name: &str) -> Option<&AliasInfo> {
        if let Some(alias) = self.type_aliases.get(name) {
            return Some(alias);
        }
        self.type_aliases
            .iter()
            .find(|(n, _)| n.ends_with(&format!("::{name}")))
            .map(|(_, a)| a)
    }

    /// Parses a type string with optional generic parameter recognition.
    fn type_from_str_impl(
        &self,
        s: &str,
        generics: &[StrId],
        strings: Option<&lasso::Rodeo>,
    ) -> Type {
        let s = s.trim();

        if let Some(strings) = strings
            && let Some(spur) = strings.get(s)
            && generics.contains(&spur)
        {
            return Type::Param(spur);
        }

        if let Some(lt_pos) = find_top_level_lt(s)
            && s.ends_with('>')
        {
            let base_name = s[..lt_pos].trim();
            let args_str = &s[lt_pos + 1..s.len() - 1];

            if let Some(alias) = self.get_type_alias(base_name) {
                let args: Vec<Type> = split_type_args(args_str)
                    .iter()
                    .map(|arg| self.type_from_str_impl(arg, generics, strings))
                    .collect();

                if args.len() != alias.generics.len() {
                    return Type::Unknown;
                }

                let mut subst = HashMap::new();
                for (g, arg) in alias.generics.iter().zip(args.iter()) {
                    if !matches!(arg, Type::Unknown | Type::InferVar(_)) {
                        subst.insert(*g, arg.clone());
                    }
                }

                let unresolved_target =
                    self.type_from_str_impl(&alias.target_ty, &alias.generics, strings);
                return Self::substitute_type(&unresolved_target, &subst);
            }

            if let Some(def_id) = self.resolve_class_name(base_name) {
                let args: Vec<Type> = split_type_args(args_str)
                    .iter()
                    .map(|arg| self.type_from_str_impl(arg, generics, strings))
                    .collect();
                return Type::Class(def_id, args);
            }
            return Type::Unknown;
        }

        if let Some(alias) = self.get_type_alias(s)
            && alias.generics.is_empty()
        {
            return self.type_from_str_impl(&alias.target_ty, &[], strings);
        }

        if let Some(def_id) = self.resolve_class_name(s) {
            return Type::Class(def_id, vec![]);
        }
        if let Some(def_id) = self.resolve_enum_name(s) {
            return Type::Enum(def_id);
        }
        Type::Unknown
    }

    /// Resolves a type string with the current class's generic parameters.
    #[must_use]
    pub fn type_from_str_with_generics(
        &self,
        s: &str,
        generics: &[StrId],
        strings: &lasso::Rodeo,
    ) -> Type {
        self.type_from_str_impl(s, generics, Some(strings))
    }

    /// Resolves a type string outside any generic context.
    #[must_use]
    pub fn type_from_str(&self, s: &str) -> Type {
        self.type_from_str_impl(s, &[], None)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Registers method across multiple lookup tables and dunder synonyms"
)]
fn insert_method_with_dunder_equivalents(
    methods: &mut HashMap<String, Rc<FunctionDecl>>,
    getters: &mut HashMap<String, Rc<FunctionDecl>>,
    setters: &mut HashMap<String, Rc<FunctionDecl>>,
    mut inline_funcs: Option<&mut HashMap<Symbol, Rc<FunctionDecl>>>,
    full_name: Option<&str>,
    short_name: &str,
    function: &FunctionDecl,
    function_rc: &Rc<FunctionDecl>,
) {
    if function.is_getter {
        getters.insert(short_name.to_owned(), Rc::clone(function_rc));
    }
    if function.is_setter {
        setters.insert(short_name.to_owned(), Rc::clone(function_rc));
    }
    methods.insert(short_name.to_owned(), Rc::clone(function_rc));
    if function.is_inline
        && let (Some(full), Some(inline_map)) = (full_name, inline_funcs.as_deref_mut())
    {
        inline_map.insert(Symbol::from(full), Rc::clone(function_rc));
    }

    let prefix = full_name.and_then(|f| f.rsplit_once("::")).map(|(p, _)| p);
    for equiv in crate::ir::dunder::dunder_equivalents(short_name) {
        if equiv != short_name {
            if function.is_getter {
                getters.insert((*equiv).to_string(), Rc::clone(function_rc));
            }
            if function.is_setter {
                setters.insert((*equiv).to_string(), Rc::clone(function_rc));
            }
            methods.insert((*equiv).to_string(), Rc::clone(function_rc));
            if function.is_inline
                && let (Some(p), Some(inline_map)) = (prefix, inline_funcs.as_deref_mut())
            {
                let equiv_full = format!("{p}::{equiv}");
                inline_map.insert(Symbol::from(equiv_full), Rc::clone(function_rc));
            }
        }
    }
}
