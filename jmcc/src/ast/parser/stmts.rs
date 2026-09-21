//! Parsing statements: branches and blocks, `return`/`break`, variable declaration
//! and assignment, action lambda blocks, and variable scopes.

use super::*;

impl Parser<'_> {
    /// Expression value after `=` if present.
    pub(super) fn parse_assign_value(&mut self) -> Result<Option<ExprId>> {
        if self.eat(Token::Assign) {
            Ok(Some(self.parse_expr(0)?))
        } else {
            Ok(None)
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_if_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        let is_not = self.eat(Token::Not);
        let condition = self.parse_expr(0)?;

        let then_body = self.parse_block_or_stmt()?;
        let mut elif_branches = Vec::new();
        let mut elif_spans = Vec::new();
        let mut else_body = None;

        loop {
            if self.eat(Token::Elif) {
                elif_spans.push(self.last_span());
                let cond = self.parse_expr(0)?;
                let block = self.parse_block_or_stmt()?;
                elif_branches.push((cond, block));
            } else if self.eat(Token::Else) {
                if self.eat(Token::If) {
                    let cond = self.parse_expr(0)?;
                    let block = self.parse_block_or_stmt()?;
                    elif_branches.push((cond, block));
                } else {
                    else_body = Some(self.parse_block_or_stmt()?);
                    break;
                }
            } else {
                break;
            }
        }

        let end_span = self.last_span().end;
        Ok(Statement::If(IfStmt {
            condition,
            then_body,
            elif_branches,
            elif_spans,
            else_body,
            is_not,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_block_or_stmt(&mut self) -> Result<Vec<Statement>> {
        if self.is_token(Token::LBrace) {
            self.parse_block()
        } else {
            Ok(vec![self.parse_statement()?])
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_while_stmt(&mut self, label: Option<StrId>) -> Result<Statement> {
        let start_span = self.span();
        self.bump();
        let is_not = self.eat(Token::Not);
        let condition = self.parse_expr(0)?;
        let body = self.parse_block_or_stmt()?;
        let end_span = self.last_span().end;
        Ok(Statement::While(WhileStmt {
            label,
            condition,
            body,
            is_not,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_for_stmt(&mut self, label: Option<StrId>) -> Result<Statement> {
        let start_span = self.span();
        self.bump();
        let has_paren = self.eat(Token::LParen);

        let mut scopes = Vec::new();
        let mut vars = Vec::new();

        let initial_scope = if self.at_var_scope() {
            Some(self.parse_var_scope()?)
        } else {
            None
        };
        let _: bool = self.eat(Token::Var) || (self.edition >= 2026 && self.eat(Token::Const));

        let mut current_scope = initial_scope;
        loop {
            if self.at_var_scope()
                && !matches!(
                    self.peek_token(),
                    Some(Token::In | Token::Comma | Token::RParen) | None
                )
            {
                current_scope = Some(self.parse_var_scope()?);
            }
            let name = self.parse_var_name()?;
            scopes.push(current_scope);
            vars.push(name);
            if !self.eat(Token::Comma) {
                break;
            }
            current_scope = None;
        }

        self.expect(Token::In)?;
        let iterable = self.parse_expr(0)?;
        if has_paren {
            self.expect(Token::RParen)?;
        }
        let body = self.parse_block_or_stmt()?;
        let end_span = self.last_span().end;

        Ok(Statement::For(ForStmt {
            label,
            vars,
            scopes,
            iterable,
            body,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_break_stmt(&mut self) -> Result<Statement> {
        let start_span = self.span();
        self.bump();
        let label = if let Some(Token::Label(lbl)) = self.curr_token() {
            let label_id = self.interner.get_or_intern(lbl);
            self.bump();
            Some(label_id)
        } else {
            None
        };
        let end_span = self.last_span().end;
        let _eaten: Result<()> = self.eat_terminator();
        Ok(Statement::Break(BreakStmt {
            label,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_continue_stmt(&mut self) -> Result<Statement> {
        let start_span = self.span();
        self.bump();
        let label = if let Some(Token::Label(lbl)) = self.curr_token() {
            let label_id = self.interner.get_or_intern(lbl);
            self.bump();
            Some(label_id)
        } else {
            None
        };
        let end_span = self.last_span().end;
        let _eaten: Result<()> = self.eat_terminator();
        Ok(Statement::Continue(ContinueStmt {
            label,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_match_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        let expr = self.parse_match_scrutinee()?;
        let arms = self.parse_match_arms()?;
        let end_span = self.last_span().end;
        Ok(Statement::Match(MatchStmt {
            expr,
            arms,
            span: start_span.start..end_span,
        }))
    }

    /// Scrutinee of `match x {`: `n {` is SNBT, so a bare ident before `{` stays an ident.
    pub(super) fn parse_match_scrutinee(&mut self) -> Result<ExprId> {
        if matches!(self.curr_token(), Some(Token::Ident(_)))
            && matches!(self.peek_token(), Some(Token::LBrace))
        {
            let start_span = self.span();
            let name = self.parse_ident_str()?;
            return Ok(self
                .arena
                .alloc(Expr::Ident(name, start_span.start..self.last_span().end)));
        }
        self.parse_expr(0)
    }

    pub(super) fn parse_match_arms(&mut self) -> Result<Vec<MatchArm>> {
        self.expect(Token::LBrace)?;
        let mut arms = Vec::new();
        while !self.is_token(Token::RBrace) && !self.is_eof() {
            arms.push(self.parse_match_arm()?);
            let _comma: bool = self.eat(Token::Comma);
        }
        self.expect(Token::RBrace)?;
        Ok(arms)
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm> {
        let start_span = self.span();
        let _case: bool = self.eat(Token::Case);
        let mut patterns = Vec::new();
        if self.eat(Token::Default) {
            patterns.push(self.wildcard_pattern(start_span.clone()));
        } else {
            // `parse_expr(0)` would eat `|` as bitwise-or and postfix `if` as a ternary.
            patterns.push(self.parse_expr(18)?);
            while self.eat(Token::BitOr) {
                patterns.push(self.parse_expr(18)?);
            }
        }
        let guard = if self.eat(Token::If) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };
        self.expect(Token::FatArrow)?;
        let body = self.parse_match_arm_body()?;
        let end_span = self.last_span().end;
        Ok(MatchArm {
            patterns,
            guard,
            body,
            span: start_span.start..end_span,
        })
    }

    fn parse_match_arm_body(&mut self) -> Result<Vec<Statement>> {
        if self.is_token(Token::LBrace) {
            return self.parse_block();
        }
        let expr = self.parse_expr(0)?;
        Ok(vec![Statement::Expr(expr)])
    }

    fn wildcard_pattern(&mut self, start_span: Span) -> ExprId {
        let name = self.interner.get_or_intern("_");
        self.arena
            .alloc(Expr::Ident(name, start_span.start..self.last_span().end))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_try_catch_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        let try_body = self.parse_block_or_stmt()?;
        self.expect(Token::Catch)?;
        let mut catch_var = None;
        let mut catch_type = None;
        if self.eat(Token::LParen) {
            if !self.is_token(Token::RParen) && !self.is_token(Token::Colon) {
                catch_var = Some(self.parse_var_name()?);
            }
            catch_type = self.parse_optional_type(Token::Colon)?;
            self.expect(Token::RParen)?;
        } else if matches!(self.curr_token(), Some(Token::Ident(_)))
            && !matches!(self.peek_token(), Some(Token::LBrace))
        {
            catch_var = Some(self.parse_var_name()?);
            catch_type = self.parse_optional_type(Token::Colon)?;
        }
        let catch_body = self.parse_block_or_stmt()?;
        let end_span = self.last_span().end;
        Ok(Statement::TryCatch(TryCatchStmt {
            try_body,
            catch_var,
            catch_type,
            catch_body,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_throw_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        let mut exception_type = None;
        let mut value = None;
        if !self.is_eof() && !self.curr_had_newline() && !self.is_token(Token::Semicolon) {
            if let Some(Token::Ident(ident)) = self.curr_token() {
                let upper = ident.to_ascii_uppercase();
                if matches!(upper.as_str(), "ERROR" | "WARNING" | "FATAL" | "ALL") {
                    exception_type = Some(self.parse_ident_str()?);
                }
            }
            if !self.is_eof() && !self.curr_had_newline() && !self.is_token(Token::Semicolon) {
                value = Some(self.parse_expr(0)?);
            }
        }
        let end_span = self.last_span().end;
        self.eat_terminator()?;
        Ok(Statement::Throw(ThrowStmt {
            value,
            exception_type,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_return_stmt(&mut self) -> Result<Statement> {
        self.bump();
        let start_span = self.span();
        let value = if !self.is_eof()
            && !self.is_token(Token::Assign)
            && !self.is_token(Token::Semicolon)
            && !self.curr_had_newline()
        {
            Some(self.parse_expr(0)?)
        } else {
            None
        };
        let end_span = self.last_span().end;
        self.eat_terminator()?;
        Ok(Statement::Return(ReturnStmt {
            value,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_var_decl_or_expr_stmt(&mut self) -> Result<Statement> {
        let start_span = self.span();

        let mut initial_scope = None;
        let is_scope = self.at_var_scope();

        if is_scope
            && matches!(
                self.peek_token(),
                Some(
                    Token::Var
                        | Token::Const
                        | Token::Ident(_)
                        | Token::DQuote
                        | Token::SQuote
                        | Token::BQuote
                        | Token::Mod
                )
            )
        {
            initial_scope = Some(self.parse_var_scope()?);
        }

        if self.eat(Token::Var) || (self.edition >= 2026 && self.eat(Token::Const)) {
            return self.parse_var_decl_rest(initial_scope, start_span);
        }

        if initial_scope.is_some() {
            return self.parse_var_decl_rest(initial_scope, start_span);
        }

        self.parse_expr_or_assign_stmt(start_span)
    }

    #[instrument(skip(self, initial_scope, start_span), level = "trace")]
    fn parse_var_decl_rest(
        &mut self,
        initial_scope: Option<VarScope>,
        start_span: Span,
    ) -> Result<Statement> {
        let mut scopes = Vec::new();
        let mut names = Vec::new();
        let mut tys = Vec::new();

        let mut current_scope = initial_scope;

        loop {
            // A scope before a later name overrides the declaration's initial scope.
            // If followed by '=', ':', ',', ';', etc., it is a variable name rather than a scope.
            if self.at_var_scope()
                && !matches!(
                    self.peek_token(),
                    Some(
                        Token::Assign
                            | Token::Colon
                            | Token::Comma
                            | Token::Semicolon
                            | Token::RBrace
                            | Token::RParen
                    ) | None
                )
            {
                current_scope = Some(self.parse_var_scope()?);
            }

            let name = self.parse_var_name()?;
            let ty = self.parse_optional_type(Token::Colon)?;

            scopes.push(current_scope);
            names.push(name);
            tys.push(ty);

            if !self.eat(Token::Comma) {
                break;
            }
            current_scope = None;
        }

        let value = self.parse_assign_value()?;

        let end_span = self.last_span().end;
        self.eat_terminator()?;

        Ok(Statement::VarDecl(VarDecl {
            scopes,
            names,
            tys,
            value,
            is_exported: false,
            span: start_span.start..end_span,
        }))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_expr_or_assign_stmt(&mut self, start_span: Span) -> Result<Statement> {
        let mut exprs = vec![self.parse_expr(0)?];
        while self.eat(Token::Comma) {
            exprs.push(self.parse_expr(0)?);
        }

        if self.eat(Token::Arrow) {
            return self.parse_lambda_block(exprs, start_span);
        }

        if self.eat(Token::Colon) {
            let ty = self.parse_type_str()?;
            let (name, scope) = match &self.arena[exprs[0]] {
                Expr::Ident(name, _) => (
                    TextValue {
                        parts: vec![TextPart::Literal(*name)],
                        parsing: TextParsing::Legacy,
                        span: self.last_span(),
                    },
                    None,
                ),
                Expr::Variable(v) => (v.name.clone(), Some(v.scope)),
                _ => return Err(JmccError::Generic("Invalid typed var decl".into())),
            };
            let value = self.parse_assign_value()?;
            let end_span = self.last_span().end;
            self.eat_terminator()?;
            return Ok(Statement::VarDecl(VarDecl {
                scopes: vec![scope],
                names: vec![name],
                tys: vec![Some(ty)],
                value,
                is_exported: false,
                span: start_span.start..end_span,
            }));
        }

        if let Some(op_str) = self.curr_assignment_op() {
            self.bump();
            let val = self.parse_expr(0)?;
            let end_span = self.last_span().end;
            self.eat_terminator()?;
            return Ok(Statement::Assign(AssignStmt {
                targets: exprs,
                op: Some(self.interner.get_or_intern(op_str)),
                value: val,
                span: start_span.start..end_span,
            }));
        }

        if let Some(last_expr_id) = exprs.last()
            && let Expr::Binary(b) = &self.arena[*last_expr_id]
            && b.op == BinOp::Assign
        {
            let val = b.right;
            let lhs = b.left;
            let mut targets = exprs.clone();
            *targets.last_mut().unwrap() = lhs;
            let end_span = self.last_span().end;
            self.eat_terminator()?;
            return Ok(Statement::Assign(AssignStmt {
                targets,
                op: Some(self.interner.get_or_intern("=")),
                value: val,
                span: start_span.start..end_span,
            }));
        }

        if self.is_token(Token::LBrace) && exprs.len() == 1 {
            return self.parse_action_block(exprs[0], start_span);
        }

        if exprs.len() != 1 {
            return Err(JmccError::Generic(
                "Unexpected comma in expression statement".into(),
            ));
        }
        self.eat_terminator()?;
        Ok(Statement::Expr(exprs[0]))
    }

    #[instrument(skip(self), level = "trace")]
    fn curr_assignment_op(&self) -> Option<&'static str> {
        match self.curr_token() {
            Some(Token::Assign) => Some("="),
            Some(Token::AddAssign) => Some("+="),
            Some(Token::SubAssign) => Some("-="),
            Some(Token::MulAssign) => Some("*="),
            Some(Token::DivAssign) => Some("/="),
            Some(Token::ModAssign) => Some("%="),
            Some(Token::PowAssign) => Some("^="),
            _ => None,
        }
    }

    #[instrument(skip(self, exprs, start_span), level = "trace")]
    fn parse_lambda_block(&mut self, exprs: Vec<ExprId>, start_span: Span) -> Result<Statement> {
        let mut lambda_params = Vec::new();
        for e in &exprs {
            if let Expr::Ident(_, _) = &self.arena[*e] {
                lambda_params.push(*e);
            } else {
                return Err(JmccError::UnexpectedToken {
                    expected: "identifier as lambda parameter".into(),
                    got: "expression".into(),
                });
            }
        }

        if !self.is_token(Token::LBrace) {
            return Err(JmccError::UnexpectedToken {
                expected: "LBrace".into(),
                got: format!("{:?}", self.curr()),
            });
        }

        let block = self.parse_block()?;
        let end_span = self.last_span().end;
        let expr = Expr::Action(ActionExpr {
            object: self.interner.get_or_intern(""),
            name: self.interner.get_or_intern(""),
            args: Vec::new(),
            operations: Some(block),
            lambda: Some(lambda_params),
            selector: None,
            invert: None,
            span: start_span.start..end_span,
        });
        Ok(Statement::Expr(self.arena.alloc(expr)))
    }

    #[instrument(skip(self, expr_id, start_span), level = "trace")]
    fn parse_action_block(&mut self, expr_id: ExprId, start_span: Span) -> Result<Statement> {
        self.expect(Token::LBrace)?;

        let mut lambda_params = Vec::new();

        let mut offset = 0;
        let mut is_lambda = false;
        loop {
            if let Some(Token::Ident(_)) = self.peek_token_at(offset) {
                if self.peek_token_at(offset + 1) == Some(Token::Comma) {
                    offset += 2;
                    continue;
                }
                if self.peek_token_at(offset + 1) == Some(Token::Arrow) {
                    is_lambda = true;
                }
            }
            break;
        }

        if is_lambda {
            while let Some(Token::Ident(s)) = self.curr_token() {
                let span = self.span();
                let sym = self.interner.get_or_intern(s);
                lambda_params.push(self.arena.alloc(Expr::Ident(sym, span)));
                self.bump();

                if self.eat(Token::Comma) {
                    continue;
                }
                if self.is_token(Token::Arrow) {
                    self.bump();
                    break;
                }
            }
        }

        let mut block_stmts = Vec::new();
        while !self.is_token(Token::RBrace) && !self.is_eof() {
            block_stmts.push(self.parse_statement()?);
        }
        self.expect(Token::RBrace)?;

        let end_span = self.last_span().end;
        let action_expr = match &self.arena[expr_id] {
            Expr::Call(call) => {
                let obj_name = match &self.arena[call.target] {
                    Expr::Ident(name, _) => *name,
                    _ => return Err(JmccError::Generic("Invalid target for action block".into())),
                };
                Expr::Action(ActionExpr {
                    object: obj_name,
                    name: call.method,
                    args: call.args.clone(),
                    operations: Some(block_stmts),
                    lambda: if lambda_params.is_empty() {
                        None
                    } else {
                        Some(lambda_params)
                    },
                    selector: None,
                    invert: None,
                    span: start_span.start..end_span,
                })
            }
            Expr::Action(act) => {
                let mut new_act = act.clone();
                new_act.operations = Some(block_stmts);
                if !lambda_params.is_empty() {
                    new_act.lambda = Some(lambda_params);
                }
                new_act.span = start_span.start..end_span;
                Expr::Action(new_act)
            }
            _ => {
                return Err(JmccError::Generic(
                    "Invalid expression before action block".into(),
                ));
            }
        };
        Ok(Statement::Expr(self.arena.alloc(action_expr)))
    }

    /// Is current token a variable scope keyword?
    ///
    /// Must match keywords recognized by [`Parser::parse_var_scope`].
    fn at_var_scope(&self) -> bool {
        matches!(
            self.curr_token(),
            Some(
                Token::Inline
                    | Token::Local
                    | Token::Game
                    | Token::Save
                    | Token::Line
                    | Token::Jmcc
            )
        )
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_var_scope(&mut self) -> Result<VarScope> {
        match self.bump_token() {
            Some(Token::Inline) => Ok(VarScope::Inline),
            Some(Token::Local) => Ok(VarScope::Local),
            Some(Token::Game) => Ok(VarScope::Game),
            Some(Token::Save) => Ok(VarScope::Save),
            Some(Token::Line) => Ok(VarScope::Line),
            Some(Token::Jmcc) => Ok(VarScope::Jmcc),
            _ => Err(JmccError::UnexpectedToken {
                expected: "var scope".into(),
                got: format!("{:?}", self.curr()),
            }),
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_var_name(&mut self) -> Result<TextValue> {
        if matches!(
            self.curr_token(),
            Some(Token::DQuote | Token::SQuote | Token::BQuote)
        ) {
            return self.parse_string(TextParsing::Legacy);
        }
        if matches!(self.curr_token(), Some(Token::Mod)) {
            let start_span = self.span();
            self.bump();
            let sym = self.parse_mod_name()?;
            return Ok(TextValue {
                parts: vec![TextPart::Literal(sym)],
                parsing: TextParsing::Legacy,
                span: start_span.start..self.last_span().end,
            });
        }

        let sym = self.parse_ident_str()?;
        Ok(TextValue {
            parts: vec![TextPart::Literal(sym)],
            parsing: TextParsing::Legacy,
            span: self.last_span(),
        })
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_var_name_spur(&mut self) -> Result<Spur> {
        let tv = self.parse_var_name()?;
        match tv.parts.first() {
            Some(TextPart::Literal(s)) if tv.parts.len() == 1 => Ok(*s),
            _ => Err(JmccError::Generic(
                "Expected simple identifier for parameter name".into(),
            )),
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_block(&mut self) -> Result<Vec<Statement>> {
        self.expect(Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.is_token(Token::RBrace) && !self.is_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(Token::RBrace)?;
        Ok(stmts)
    }
}
