//! Parsing declarations: program and top-level statements, functions, processes,
//! events, classes, enums, type aliases, imports/exports, parameters, and generics.

use super::*;

#[derive(Clone)]
struct ExportModifiers {
    is_getter: bool,
    is_setter: bool,
    lang_item: bool,
    is_dict: bool,
    is_overload: bool,
    aliases: Vec<StrId>,
    test_attr: Option<TestAttribute>,
}

impl Parser<'_> {
    /// # Errors
    ///
    /// Returns an error when the token stream does not form a valid program.
    #[instrument(skip(self), level = "info")]
    pub fn parse_program(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while !self.is_eof() {
            let stmt = self.parse_statement()?;
            trace!("Parsed statement: {stmt:?}");
            stmts.push(stmt);
        }
        Ok(stmts)
    }

    #[instrument(skip(self), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "parsing top level statements and decorators"
    )]
    pub(super) fn parse_statement(&mut self) -> Result<Statement> {
        while self.eat(Token::Semicolon) {}
        let mut is_getter = false;
        let mut is_setter = false;
        let mut lang_item = false;
        let mut is_dict = false;
        let mut is_overload = false;
        let mut aliases = Vec::new();
        let mut is_test = false;
        let mut should_panic = false;
        let mut expected_panic = None;
        let mut is_ignore = false;
        let mut ignore_reason = None;
        let mut cfgs = Vec::new();

        while self.is_token(Token::At) {
            self.bump();
            let name = self.parse_ident_str()?;
            let name_str = self.interner.resolve(&name);
            trace!("Parsed decorator: {name_str}");
            match name_str {
                "cfg" | "кфг" => {
                    let start_span = self.last_span();
                    let cfg_expr = self.parse_cfg_decorator()?;
                    let end_span = self.last_span().end;
                    cfgs.push(CfgAttr {
                        expr: cfg_expr,
                        span: start_span.start..end_span,
                    });
                }
                "getter" | "геттер" => is_getter = true,
                "setter" | "сеттер" => is_setter = true,
                "overload" | "перегрузка" => is_overload = true,
                "lang_item" | "языковой_элемент" => {
                    // The optional decorator argument does not affect semantic analysis.
                    if self.eat(Token::LParen) {
                        let _parsed: Result<Spur> = self.parse_string_value();
                        self.expect(Token::RParen)?;
                    }
                    lang_item = true;
                }
                "dict" | "словарь" => is_dict = true,
                "alias" | "алиас" => {
                    let parsed = self.parse_alias_decorator()?;
                    aliases.extend(parsed);
                }
                "test" | "тест" => is_test = true,
                "should_panic" | "должна_паниковать" | "должно_паниковать" =>
                {
                    should_panic = true;
                    if self.eat(Token::LParen) {
                        let val: Result<Spur> = self.parse_string_value();
                        if let Ok(s) = val {
                            expected_panic = Some(self.interner.resolve(&s).to_owned());
                        }
                        self.expect(Token::RParen)?;
                    }
                }
                "ignore" | "игнорировать" => {
                    is_ignore = true;
                    if self.eat(Token::LParen) {
                        let val: Result<Spur> = self.parse_string_value();
                        if let Ok(s) = val {
                            ignore_reason = Some(self.interner.resolve(&s).to_owned());
                        }
                        self.expect(Token::RParen)?;
                    }
                }
                _ => {}
            }
        }

        let test_attr = (is_test || should_panic || is_ignore).then_some(TestAttribute {
            is_test,
            should_panic,
            expected_panic,
            is_ignore,
            ignore_reason,
        });

        let mut stmt = if self.is_token(Token::Export) {
            self.parse_export_stmt(ExportModifiers {
                is_getter,
                is_setter,
                lang_item,
                is_dict,
                is_overload,
                aliases,
                test_attr,
            })?
        } else {
            let label = if let Some(Token::LabelDecl(lbl)) = self.curr_token() {
                let label_id = self.interner.get_or_intern(lbl);
                self.bump();
                Some(label_id)
            } else {
                None
            };

            match self.curr_token() {
                Some(Token::Import) => self.parse_import_stmt()?,
                Some(Token::Function | Token::Fun | Token::Def) => self.parse_function_decl(
                    false,
                    is_getter,
                    is_setter,
                    is_overload,
                    aliases,
                    test_attr,
                )?,
                Some(Token::Process) => self.parse_process_stmt(aliases)?,
                Some(Token::Event) => self.parse_event_stmt()?,
                Some(Token::Class) => {
                    self.parse_class_stmt(false, false, lang_item, is_dict, aliases)?
                }
                Some(Token::Interface) => self.parse_interface_stmt(aliases)?,
                Some(Token::Enum) => self.parse_enum_stmt(false, aliases)?,
                Some(Token::TypeAlias) => self.parse_type_alias_stmt(aliases)?,
                Some(Token::If) => self.parse_if_stmt()?,
                Some(Token::While) => self.parse_while_stmt(label)?,
                Some(Token::For) => self.parse_for_stmt(label)?,
                Some(Token::Match) => self.parse_match_stmt()?,
                Some(Token::Try) => self.parse_try_catch_stmt()?,
                Some(Token::Throw) => self.parse_throw_stmt()?,
                Some(Token::Break) => self.parse_break_stmt()?,
                Some(Token::Return | Token::Continue)
                    if self.edition < 2026
                        && matches!(
                            self.peek_token(),
                            Some(
                                Token::Assign
                                    | Token::AddAssign
                                    | Token::SubAssign
                                    | Token::MulAssign
                                    | Token::DivAssign
                                    | Token::ModAssign
                                    | Token::PowAssign
                            )
                        ) =>
                {
                    self.parse_var_decl_or_expr_stmt()?
                }
                Some(Token::Return) => self.parse_return_stmt()?,
                Some(Token::Continue) => self.parse_continue_stmt()?,
                Some(Token::Inline | Token::Cached) => {
                    let is_modifier = matches!(
                        self.peek_token(),
                        Some(
                            Token::Function
                                | Token::Fun
                                | Token::Def
                                | Token::Class
                                | Token::Inline
                                | Token::Cached
                                | Token::Var
                                | Token::Const
                                | Token::Local
                                | Token::Line
                                | Token::Game
                                | Token::Save
                                | Token::Ident(_)
                                | Token::DQuote
                                | Token::SQuote
                                | Token::BQuote
                                | Token::Mod
                        )
                    );
                    if !is_modifier {
                        self.parse_var_decl_or_expr_stmt()?
                    } else {
                        let mut is_inline = false;
                        let mut is_cached = false;
                        while matches!(self.curr_token(), Some(Token::Inline | Token::Cached)) {
                            if self.eat(Token::Inline) {
                                is_inline = true;
                            } else if self.eat(Token::Cached) {
                                is_cached = true;
                            }
                        }
                        if matches!(
                            self.curr_token(),
                            Some(Token::Function | Token::Fun | Token::Def)
                        ) {
                            self.parse_function_decl(
                                is_inline,
                                is_getter,
                                is_setter,
                                is_overload,
                                aliases,
                                test_attr,
                            )?
                        } else if matches!(self.curr_token(), Some(Token::Class)) {
                            self.parse_class_stmt(
                                is_inline, is_cached, lang_item, is_dict, aliases,
                            )?
                        } else {
                            self.parse_var_decl_or_expr_stmt_with_cached(is_cached)?
                        }
                    }
                }
                _ => self.parse_var_decl_or_expr_stmt()?,
            }
        };

        for cfg in cfgs.into_iter().rev() {
            stmt = Statement::Cfg(cfg, Box::new(stmt));
        }
        Ok(stmt)
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_type_alias_stmt(&mut self, aliases: Vec<StrId>) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        let generics = self.parse_generics()?;

        self.expect(Token::Assign)?;
        let target_ty = self.parse_type_str()?;
        let end_span = self.last_span().end;
        self.eat_terminator()?;

        Ok(Statement::TypeAlias(TypeAliasDecl {
            name,
            generics,
            target_ty,
            is_exported: false,
            aliases,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self, modifiers), level = "trace")]
    fn parse_export_stmt(&mut self, mut modifiers: ExportModifiers) -> Result<Statement> {
        self.bump();
        let mut is_inline = false;
        let mut is_cached = false;
        while matches!(self.curr_token(), Some(Token::Inline | Token::Cached)) {
            if self.eat(Token::Inline) {
                is_inline = true;
            } else if self.eat(Token::Cached) {
                is_cached = true;
            }
        }
        let mut export_cfgs = Vec::new();

        while self.is_token(Token::At) {
            self.bump();
            let name = self.parse_ident_str()?;
            let name_str = self.interner.resolve(&name);
            match name_str {
                "cfg" | "кфг" => {
                    let start_span = self.last_span();
                    let cfg_expr = self.parse_cfg_decorator()?;
                    let end_span = self.last_span().end;
                    export_cfgs.push(CfgAttr {
                        expr: cfg_expr,
                        span: start_span.start..end_span,
                    });
                }
                "getter" | "геттер" => modifiers.is_getter = true,
                "setter" | "сеттер" => modifiers.is_setter = true,
                "overload" | "перегрузка" => modifiers.is_overload = true,
                "alias" | "алиас" => {
                    let parsed = self.parse_alias_decorator()?;
                    modifiers.aliases.extend(parsed);
                }
                _ => {}
            }
        }

        let mut stmt = match self.curr_token() {
            Some(Token::Function | Token::Fun | Token::Def) => self.parse_function_decl(
                is_inline,
                modifiers.is_getter,
                modifiers.is_setter,
                modifiers.is_overload,
                modifiers.aliases,
                modifiers.test_attr,
            )?,
            Some(Token::Process) => self.parse_process_stmt(modifiers.aliases)?,
            Some(Token::Var | Token::Const) => {
                self.parse_var_decl_or_expr_stmt_with_cached(is_cached)?
            }
            Some(Token::Class) => self.parse_class_stmt(
                is_inline,
                is_cached,
                modifiers.lang_item,
                modifiers.is_dict,
                modifiers.aliases,
            )?,
            Some(Token::Interface) => self.parse_interface_stmt(modifiers.aliases)?,
            Some(Token::TypeAlias) => self.parse_type_alias_stmt(modifiers.aliases)?,
            Some(Token::Enum) => self.parse_enum_stmt(true, modifiers.aliases)?,
            _ if is_cached => self.parse_var_decl_or_expr_stmt_with_cached(is_cached)?,
            _ => {
                return Err(JmccError::UnexpectedToken {
                    expected: "declaration after 'export'".into(),
                    got: format!("{:?}", self.curr()),
                });
            }
        };

        match &mut stmt {
            Statement::Function(f) => f.is_exported = true,
            Statement::Process(p) => p.is_exported = true,
            Statement::VarDecl(v) => v.is_exported = true,
            Statement::Class(c) => c.is_exported = true,
            Statement::Interface(i) => i.is_exported = true,
            Statement::TypeAlias(ta) => ta.is_exported = true,
            Statement::Enum(e) => e.is_exported = true,
            _ => {
                return Err(JmccError::Generic(
                    "Can only export functions, processes, variables, classes, interfaces, or type aliases"
                        .into(),
                ));
            }
        }

        for cfg in export_cfgs.into_iter().rev() {
            stmt = Statement::Cfg(cfg, Box::new(stmt));
        }
        Ok(stmt)
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_import_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();

        let parse_simple_path = |s: &mut Self| -> Result<Statement> {
            let path = s.parse_string_value()?;
            let end_span = s.last_span().end;
            s.eat_terminator()?;
            Ok(Statement::Import(ImportStmt {
                path,
                kind: ImportKind::SideEffect,
                span: start_span.start..end_span,
            }))
        };

        if self.edition < 2026
            || matches!(
                self.curr_token(),
                Some(Token::DQuote | Token::SQuote | Token::BQuote)
            )
        {
            return parse_simple_path(self);
        }

        let kind = if self.is_token(Token::Mul) {
            self.bump();
            self.expect(Token::As)?;
            ImportKind::Namespace(self.parse_ident_str()?)
        } else if self.is_token(Token::LBrace) {
            self.bump();
            let items = self.parse_comma_separated(Token::RBrace, |s| {
                let original = s.parse_ident_str()?;
                let local = if s.is_token(Token::As) {
                    s.bump();
                    s.parse_ident_str()?
                } else {
                    original
                };
                Ok(ImportItem { original, local })
            })?;
            self.expect(Token::RBrace)?;
            ImportKind::Named(items)
        } else {
            ImportKind::Default(self.parse_ident_str()?)
        };

        self.expect(Token::From)?;
        let path = self.parse_string_value()?;
        let end_span = self.last_span().end;
        self.eat_terminator()?;
        Ok(Statement::Import(ImportStmt {
            path,
            kind,
            span: start_span.start..end_span,
        }))
    }

    fn parse_alias_decorator(&mut self) -> Result<Vec<StrId>> {
        self.expect(Token::LParen)?;
        let mut res = Vec::new();
        while !self.is_token(Token::RParen) && !self.is_eof() {
            if self.peek_token() == Some(Token::Assign) {
                let _lang = self.parse_ident_str()?;
                self.expect(Token::Assign)?;
            }
            let target_str_id = self.parse_alias_target()?;
            res.push(target_str_id);
            if !self.eat(Token::Comma) {
                break;
            }
        }
        self.expect(Token::RParen)?;
        Ok(res)
    }

    fn parse_alias_target(&mut self) -> Result<StrId> {
        if matches!(
            self.curr_token(),
            Some(Token::DQuote | Token::SQuote | Token::BQuote)
        ) {
            self.parse_string_value()
        } else {
            self.parse_ident_str()
        }
    }

    fn parse_cfg_decorator(&mut self) -> Result<CfgExpr> {
        self.expect(Token::LParen)?;
        let expr = self.parse_cfg_expr()?;
        self.expect(Token::RParen)?;
        Ok(expr)
    }

    fn parse_cfg_expr(&mut self) -> Result<CfgExpr> {
        if matches!(
            self.curr_token(),
            Some(Token::DQuote | Token::SQuote | Token::BQuote)
        ) {
            let s = self.parse_string_value()?;
            let feat_str = self.interner.resolve(&s).to_owned();
            return Ok(CfgExpr::Feature(feat_str));
        }

        let ident = self.parse_ident_str()?;
        let ident_str = self.interner.resolve(&ident).to_owned();
        match ident_str.as_str() {
            "feature" | "фича" => {
                if !self.eat(Token::Assign) && !self.eat(Token::Colon) {
                    return Err(JmccError::Generic(format!(
                        "Expected '=' or ':' after '{ident_str}' in @cfg"
                    )));
                }
                let feat_str = if matches!(
                    self.curr_token(),
                    Some(Token::DQuote | Token::SQuote | Token::BQuote)
                ) {
                    let s = self.parse_string_value()?;
                    self.interner.resolve(&s).to_owned()
                } else {
                    let s = self.parse_ident_str()?;
                    self.interner.resolve(&s).to_owned()
                };
                Ok(CfgExpr::Feature(feat_str))
            }
            "not" | "не" => {
                self.expect(Token::LParen)?;
                let inner = self.parse_cfg_expr()?;
                self.expect(Token::RParen)?;
                Ok(CfgExpr::Not(Box::new(inner)))
            }
            "all" | "все" => {
                self.expect(Token::LParen)?;
                let mut exprs = Vec::new();
                while !self.is_token(Token::RParen) && !self.is_eof() {
                    exprs.push(self.parse_cfg_expr()?);
                    if !self.eat(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                Ok(CfgExpr::All(exprs))
            }
            "any" | "любой" | "любая" => {
                self.expect(Token::LParen)?;
                let mut exprs = Vec::new();
                while !self.is_token(Token::RParen) && !self.is_eof() {
                    exprs.push(self.parse_cfg_expr()?);
                    if !self.eat(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                Ok(CfgExpr::Any(exprs))
            }
            other => Err(JmccError::Generic(format!(
                "Unknown @cfg parameter or predicate '{other}', expected 'feature' (or 'фича')"
            ))),
        }
    }

    #[instrument(
        skip(self, is_inline, is_getter, is_setter, is_overload, aliases, test_attr),
        level = "trace"
    )]
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "declaration flags are parsed independently from decorators and keywords"
    )]
    fn parse_function_decl(
        &mut self,
        is_inline: bool,
        is_getter: bool,
        is_setter: bool,
        is_overload: bool,
        aliases: Vec<StrId>,
        test_attr: Option<TestAttribute>,
    ) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        let generics = self.parse_generics()?;
        let params = self.parse_paren_params()?;
        let return_type = self.parse_optional_type(Token::Arrow)?;
        let body = if self.eat(Token::Semicolon) {
            Vec::new()
        } else {
            self.parse_block()?
        };
        let end_span = self.last_span().end;
        Ok(Statement::Function(FunctionDecl {
            name,
            generics,
            params,
            return_type,
            body,
            is_inline,
            is_exported: false,
            is_getter,
            is_setter,
            is_overload,
            aliases,
            test_attr,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_paren_params(&mut self) -> Result<Vec<Param>> {
        if self.eat(Token::LParen) {
            let p = self.parse_params()?;
            self.expect(Token::RParen)?;
            Ok(p)
        } else {
            Ok(Vec::new())
        }
    }

    /// Consumes the declaration keyword and parses its name.
    ///
    /// Span starts at the name, not the keyword, matching traditional parser conventions.
    fn parse_decl_name(&mut self) -> Result<(Span, Spur)> {
        self.bump();
        let start_span = self.span();
        Ok((start_span, self.parse_string_value()?))
    }

    /// Parses `<T, U, ...>`; returns empty vector if no `<` is present.
    fn parse_generics(&mut self) -> Result<Vec<Spur>> {
        if !self.eat(Token::Lt) {
            return Ok(Vec::new());
        }
        let generics = self.parse_comma_separated(Token::Gt, Self::parse_ident_str)?;
        self.expect(Token::Gt)?;
        Ok(generics)
    }

    #[instrument(skip(self, aliases), level = "trace")]
    fn parse_process_stmt(&mut self, aliases: Vec<StrId>) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        let params = self.parse_paren_params()?;
        let body = self.parse_block()?;
        let end_span = self.last_span().end;
        Ok(Statement::Process(ProcessDecl {
            name,
            params,
            body,
            is_exported: false,
            aliases,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_event_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        self.expect(Token::Lt)?;
        let event_name = self.parse_ident_str()?;
        self.expect(Token::Gt)?;
        let body = self.parse_block()?;
        let end_span = self.last_span().end;
        Ok(Statement::Event(EventDecl {
            event_name,
            body,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(
        skip(self, is_inline, is_cached, lang_item, is_dict, aliases),
        level = "trace"
    )]
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "decorator flags for class definition"
    )]
    fn parse_class_stmt(
        &mut self,
        is_inline: bool,
        is_cached: bool,
        lang_item: bool,
        is_dict: bool,
        aliases: Vec<StrId>,
    ) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        let generics = self.parse_generics()?;

        let parent = if self.eat(Token::Extends) {
            Some(self.parse_ident_str()?)
        } else if self.eat(Token::LParen) {
            let p = self.parse_ident_str()?;
            self.expect(Token::RParen)?;
            Some(p)
        } else {
            None
        };

        let mut implements = Vec::new();
        if self.eat(Token::Implements) || (parent.is_none() && self.eat(Token::Colon)) {
            loop {
                implements.push(self.parse_type_str()?);
                if !self.eat(Token::Comma) {
                    break;
                }
            }
        }

        let body = self.parse_block()?;
        let end_span = self.last_span().end;
        Ok(Statement::Class(ClassDecl {
            name,
            generics,
            parent,
            implements,
            body,
            is_inline,
            is_cached,
            lang_item,
            is_dict,
            is_exported: false,
            aliases,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self, aliases), level = "trace")]
    fn parse_interface_stmt(&mut self, aliases: Vec<StrId>) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        let generics = self.parse_generics()?;
        let mut parents = Vec::new();
        if self.eat(Token::Extends) || self.eat(Token::Colon) {
            loop {
                parents.push(self.parse_type_str()?);
                if !self.eat(Token::Comma) {
                    break;
                }
            }
        } else if self.eat(Token::LParen) {
            loop {
                parents.push(self.parse_type_str()?);
                if !self.eat(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RParen)?;
        }
        let body = self.parse_block()?;
        let end_span = self.last_span().end;
        Ok(Statement::Interface(InterfaceDecl {
            name,
            generics,
            parents,
            body,
            is_exported: false,
            aliases,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self, aliases), level = "trace")]
    fn parse_enum_stmt(&mut self, is_exported: bool, aliases: Vec<StrId>) -> Result<Statement> {
        let (start_span, name) = self.parse_decl_name()?;
        self.expect(Token::LBrace)?;
        let values = self.parse_comma_separated(Token::RBrace, Self::parse_ident_str)?;
        self.expect(Token::RBrace)?;
        let end_span = self.last_span().end;
        Ok(Statement::Enum(EnumDecl {
            name,
            values,
            is_exported,
            aliases,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_params(&mut self) -> Result<Vec<Param>> {
        self.parse_comma_separated(Token::RParen, |s| {
            let start_span = s.span();
            let is_ref = s.eat(Token::Ref);
            let mut spread = 0;
            if s.eat(Token::DoubleStar) {
                spread = 2;
            } else if s.eat(Token::Mul) {
                spread = 1;
            }

            let name = s.parse_var_name_spur()?;
            let ty = s.parse_optional_type(Token::Colon)?;
            let default = s.parse_assign_value()?;
            let end_span = s.last_span().end;

            Ok(Param {
                name,
                ty,
                default,
                is_ref,
                spread,
                span: start_span.start..end_span,
            })
        })
    }
}
