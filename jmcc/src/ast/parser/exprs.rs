//! Parsing expressions: Pratt parser, prefix and postfix operators,
//! list/map/NBT literals, calls, indexing, properties, and type casting.

use super::*;

enum PostOp {
    Property,
    Action,
    Subscript,
    Call,
    PostInc,
    PostDec,
}

impl Parser<'_> {
    #[instrument(skip(self, min_bp), level = "trace")]
    pub(super) fn parse_expr(&mut self, min_bp: u8) -> Result<ExprId> {
        trace!("Parsing expr with min_bp={min_bp}");
        let start_span = self.span();
        let mut lhs = self.parse_primary(min_bp)?;
        lhs = self.parse_postfix_ops(lhs, min_bp)?;

        if self.is_token(Token::Question) && min_bp <= 1 {
            self.bump();
            let then_val = self.parse_expr(0)?;
            self.expect(Token::Colon)?;
            let else_val = self.parse_expr(0)?;
            let end_span = self.arena[else_val].span().end;
            let expr = Expr::Ternary(TernaryExpr {
                cond: lhs,
                then_val,
                else_val,
                span: start_span.start..end_span,
            });
            lhs = self.arena.alloc(expr);
            return self.parse_infix_ops(lhs, min_bp, start_span);
        }

        if self.is_token(Token::If) && min_bp <= 1 {
            // A line break before `if` starts a statement instead of a ternary expression.
            if self.curr_had_newline() {
                return Ok(lhs);
            }
            if matches!(self.peek_token(), Some(Token::LBrace)) {
                return Ok(lhs);
            }
            self.bump();
            let cond = self.parse_expr(0)?;
            self.expect(Token::Else)?;
            let else_val = self.parse_expr(0)?;
            let end_span = self.arena[else_val].span().end;
            let expr = Expr::Ternary(TernaryExpr {
                cond,
                then_val: lhs,
                else_val,
                span: start_span.start..end_span,
            });
            lhs = self.arena.alloc(expr);
            return self.parse_infix_ops(lhs, min_bp, start_span);
        }

        if min_bp == 0 && self.is_token(Token::Assign) {
            self.bump();
            let rhs = self.parse_expr(0)?;
            let span = self.arena[lhs].span().start..self.last_span().end;
            let bin = Expr::Binary(BinaryExpr {
                op: BinOp::Assign,
                left: lhs,
                right: rhs,
                span,
            });
            lhs = self.arena.alloc(bin);
        }

        self.parse_infix_ops(lhs, min_bp, start_span)
    }

    #[instrument(skip(self, lhs, min_bp), level = "trace")]
    fn parse_postfix_ops(&mut self, mut lhs: ExprId, min_bp: u8) -> Result<ExprId> {
        while let Some(t) = self.curr_token() {
            let (op, l_bp, _r_bp) = match t {
                Token::Dot => (PostOp::Property, 26, 27),
                Token::ScopeRes => (PostOp::Action, 26, 27),
                Token::LBracket => (PostOp::Subscript, 26, 27),
                Token::LParen => (PostOp::Call, 26, 27),
                Token::Inc => (PostOp::PostInc, 28, 29),
                Token::Dec => (PostOp::PostDec, 28, 29),
                _ => break,
            };

            if l_bp < min_bp {
                break;
            }

            self.bump();
            let lhs_start = self.arena[lhs].span().start;

            lhs = match op {
                PostOp::Property => self.parse_property_op(lhs, lhs_start)?,
                PostOp::Action => self.parse_action_op(lhs, lhs_start)?,
                PostOp::Subscript => self.parse_subscript_op(lhs, lhs_start)?,
                PostOp::Call => self.parse_call_op(lhs, lhs_start)?,
                PostOp::PostInc => {
                    let span = lhs_start..self.last_span().end;
                    self.arena.alloc(Expr::Unary(UnaryExpr {
                        op: UnOp::Inc,
                        operand: lhs,
                        span,
                    }))
                }
                PostOp::PostDec => {
                    let span = lhs_start..self.last_span().end;
                    self.arena.alloc(Expr::Unary(UnaryExpr {
                        op: UnOp::Dec,
                        operand: lhs,
                        span,
                    }))
                }
            };
        }
        Ok(lhs)
    }

    #[instrument(skip(self, lhs, start), level = "trace")]
    fn parse_property_op(&mut self, lhs: ExprId, start: usize) -> Result<ExprId> {
        let name = self.parse_ident_str()?;
        let span = start..self.last_span().end;
        Ok(self.arena.alloc(Expr::Property(PropertyExpr {
            object: lhs,
            property: name,
            span,
        })))
    }

    #[instrument(skip(self, lhs, start), level = "trace")]
    fn parse_action_op(&mut self, lhs: ExprId, start: usize) -> Result<ExprId> {
        let name = self.parse_ident_str()?;
        let selector = if self.eat(Token::Lt) {
            let s_start = self.span().start;
            let mut s_end = s_start;
            while !self.is_token(Token::Gt) && !self.is_eof() {
                s_end = self.span().end;
                self.bump();
            }
            if self.is_eof() {
                return Err(JmccError::UnexpectedEof {
                    context: "action selector".into(),
                });
            }
            self.expect(Token::Gt)?;
            Some(
                self.interner
                    .get_or_intern(&self.input[s_start - self.offset..s_end - self.offset]),
            )
        } else {
            None
        };

        let args = if self.eat(Token::LParen) {
            let a = self.parse_args()?;
            self.expect(Token::RParen)?;
            a
        } else {
            Vec::new()
        };

        let obj_name = match &self.arena[lhs] {
            Expr::Ident(name, _) => *name,
            _ => return Err(JmccError::Generic("Invalid target for action call".into())),
        };

        let span = start..self.last_span().end;
        let expr = Expr::Action(ActionExpr {
            object: obj_name,
            name,
            args,
            operations: None,
            lambda: None,
            selector,
            invert: None,
            span,
        });
        Ok(self.arena.alloc(expr))
    }

    #[instrument(skip(self, lhs, start), level = "trace")]
    fn parse_subscript_op(&mut self, lhs: ExprId, start: usize) -> Result<ExprId> {
        let index = self.parse_expr(0)?;
        let end = if self.eat(Token::Colon) && !self.is_token(Token::RBracket) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };
        self.expect(Token::RBracket)?;
        let span = start..self.last_span().end;
        Ok(self.arena.alloc(Expr::Subscript(SubscriptExpr {
            object: lhs,
            index,
            end,
            span,
        })))
    }

    #[instrument(skip(self, lhs, start), level = "trace")]
    fn parse_call_op(&mut self, lhs: ExprId, start: usize) -> Result<ExprId> {
        let args = self.parse_args()?;
        self.expect(Token::RParen)?;
        let span = start..self.last_span().end;

        let lhs_expr = self.arena[lhs].clone();
        match lhs_expr {
            Expr::Property(prop) => {
                let expr = Expr::Call(CallExpr {
                    target: prop.object,
                    method: prop.property,
                    args,
                    span,
                });
                Ok(self.arena.alloc(expr))
            }
            Expr::Ident(name, _) => {
                let name_str = self.interner.resolve(&name);
                let (is_builtin, ctor_name) = match name_str {
                    "sound" | "звук" => (true, self.interner.get_or_intern("sound")),
                    "particle" | "частица" => {
                        (true, self.interner.get_or_intern("particle"))
                    }
                    "potion" | "зелье" => (true, self.interner.get_or_intern("potion")),
                    "item" | "предмет" => (true, self.interner.get_or_intern("item")),
                    "block" | "блок" => (true, self.interner.get_or_intern("block")),
                    "value" | "значение" => (true, self.interner.get_or_intern("value")),
                    "enum" | "перечисление" => {
                        (true, self.interner.get_or_intern("enum"))
                    }
                    _ => (false, name),
                };
                if is_builtin {
                    Ok(self.arena.alloc(Expr::Constructor(ConstructorExpr {
                        name: ctor_name,
                        args,
                        span,
                    })))
                } else {
                    Ok(self.arena.alloc(Expr::Call(CallExpr {
                        target: lhs,
                        method: name,
                        args,
                        span,
                    })))
                }
            }
            _ => {
                let method = self.interner.get_or_intern("call");
                Ok(self.arena.alloc(Expr::Call(CallExpr {
                    target: lhs,
                    method,
                    args,
                    span,
                })))
            }
        }
    }

    #[instrument(skip(self, lhs, min_bp, start_span), level = "trace")]
    fn parse_infix_ops(&mut self, mut lhs: ExprId, min_bp: u8, start_span: Span) -> Result<ExprId> {
        while let Some(t) = self.curr_token() {
            if matches!(t, Token::As) {
                if 23 < min_bp {
                    break;
                }
                self.bump();
                let ty = self.parse_type_str()?;
                let end_span = self.last_span().end;
                let expr = Expr::Cast(CastExpr {
                    expr: lhs,
                    ty,
                    span: start_span.start..end_span,
                });
                lhs = self.arena.alloc(expr);
                continue;
            }

            let (op, l_bp, r_bp) = match t {
                Token::Or => (BinOp::Or, 1, 2),
                Token::And => (BinOp::And, 3, 4),
                Token::Eq => (BinOp::Eq, 5, 6),
                Token::Ne => (BinOp::Ne, 5, 6),
                Token::Le => (BinOp::Le, 7, 8),
                Token::Ge => (BinOp::Ge, 7, 8),
                Token::Lt => (BinOp::Lt, 7, 8),
                Token::Gt => (BinOp::Gt, 7, 8),
                Token::In => (BinOp::In, 7, 8),
                Token::DotDot => (BinOp::Range, 9, 10),
                Token::DotDotEq => (BinOp::RangeInclusive, 9, 10),
                Token::Add => (BinOp::Add, 11, 12),
                Token::Sub => (BinOp::Sub, 11, 12),
                Token::Mul => (BinOp::Mul, 13, 14),
                Token::Div => (BinOp::Div, 13, 14),
                Token::Mod => {
                    if matches!(self.peek_token(), Some(Token::Ident(_)))
                        && self.peek_token_at(2) == Some(Token::Mod)
                    {
                        break;
                    }
                    (BinOp::Mod, 13, 14)
                }
                Token::BitAnd => (BinOp::BitAnd, 15, 16),
                Token::BitOr => (BinOp::BitOr, 17, 18),
                Token::Shl => (BinOp::Shl, 19, 20),
                Token::Shr => (BinOp::Shr, 19, 20),
                Token::Pow => (BinOp::Pow, 21, 20),
                _ => break,
            };

            if l_bp < min_bp {
                break;
            }

            self.bump();
            let rhs = self.parse_expr(r_bp)?;
            let end_span = self.last_span().end;
            let expr = Expr::Binary(BinaryExpr {
                op,
                left: lhs,
                right: rhs,
                span: start_span.start..end_span,
            });
            lhs = self.arena.alloc(expr);
        }
        Ok(lhs)
    }

    #[instrument(skip(self, min_bp), level = "trace")]
    fn parse_primary(&mut self, min_bp: u8) -> Result<ExprId> {
        let start_span = self.span();

        if let Some(expr_id) = self.parse_prefix_op(start_span.clone())? {
            return Ok(expr_id);
        }

        if min_bp == 0 && self.is_token(Token::LParen) && self.is_lambda_at_lparen() {
            return self.parse_lambda_paren_expr(start_span);
        }

        if min_bp == 0
            && matches!(self.curr_token(), Some(Token::Ident(_)))
            && self.peek_token() == Some(Token::FatArrow)
        {
            return self.parse_lambda_single_ident_expr(start_span);
        }

        if self.eat(Token::LParen) {
            let expr = self.parse_expr(0)?;
            self.expect(Token::RParen)?;
            return Ok(expr);
        }

        if self.eat(Token::LBracket) {
            return self.parse_list_literal(start_span);
        }

        if self.eat(Token::LBrace) {
            return self.parse_map_literal(start_span);
        }

        if self.eat(Token::Match) {
            let expr = self.parse_match_scrutinee()?;
            let arms = self.parse_match_arms()?;
            let span = start_span.start..self.last_span().end;
            return Ok(self
                .arena
                .alloc(Expr::Match(MatchExpr { expr, arms, span })));
        }

        if self.eat(Token::If) {
            let cond = self.parse_expr(0)?;
            let then_val = self.parse_expr(0)?;
            self.expect(Token::Else)?;
            let else_val = self.parse_expr(0)?;
            let span = start_span.start..self.last_span().end;
            let expr = Expr::Ternary(TernaryExpr {
                cond,
                then_val,
                else_val,
                span,
            });
            return Ok(self.arena.alloc(expr));
        }

        if self.eat(Token::Mod) {
            return self.parse_mod_ident(start_span);
        }

        if let Some(expr_id) = self.parse_typed_literal(start_span.clone())? {
            return Ok(expr_id);
        }

        if matches!(self.curr_token(), Some(Token::BQuote)) {
            let tv = self.parse_string(TextParsing::Legacy)?;
            return Ok(self.arena.alloc(Expr::Variable(VariableExpr {
                name: tv,
                scope: self.default_scope,
                value_type: None,
                span: start_span,
            })));
        }

        if matches!(self.curr_token(), Some(Token::DQuote | Token::SQuote)) {
            let tv = self.parse_string(TextParsing::Legacy)?;
            return Ok(self.arena.alloc(Expr::Text(tv)));
        }

        self.parse_basic_literal(start_span)
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_prefix_op(&mut self, start_span: Span) -> Result<Option<ExprId>> {
        let op = match self.curr_token() {
            Some(Token::Not) => UnOp::Not,
            Some(Token::Sub) => UnOp::Neg,
            Some(Token::Inc) => UnOp::Inc,
            Some(Token::Dec) => UnOp::Dec,
            _ => return Ok(None),
        };
        self.bump();
        let operand = self.parse_expr(21)?;
        let span = start_span.start..self.last_span().end;
        Ok(Some(self.arena.alloc(Expr::Unary(UnaryExpr {
            op,
            operand,
            span,
        }))))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_list_literal(&mut self, start_span: Span) -> Result<ExprId> {
        let values = self.parse_comma_separated(Token::RBracket, |s| s.parse_expr(0))?;
        self.expect(Token::RBracket)?;
        let span = start_span.start..self.last_span().end;
        Ok(self.arena.alloc(Expr::List(ListExpr { values, span })))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_map_literal(&mut self, start_span: Span) -> Result<ExprId> {
        let pairs = self.parse_comma_separated(Token::RBrace, |s| {
            let key = s.parse_expr(0)?;
            s.expect(Token::Colon)?;
            Ok((key, s.parse_expr(0)?))
        })?;
        self.expect(Token::RBrace)?;
        let (keys, values) = pairs.into_iter().unzip();
        let span = start_span.start..self.last_span().end;
        Ok(self.arena.alloc(Expr::Map(MapExpr { keys, values, span })))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_mod_ident(&mut self, start_span: Span) -> Result<ExprId> {
        let name = self.parse_mod_name()?;
        Ok(self.arena.alloc(Expr::Ident(name, start_span)))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_typed_literal(&mut self, start_span: Span) -> Result<Option<ExprId>> {
        let Some(Token::Ident(s)) = self.curr_token() else {
            return Ok(None);
        };

        let next_span = self.peek_span();
        let is_adjacent = next_span
            .as_ref()
            .is_some_and(|sp| sp.start == start_span.end);
        if !is_adjacent {
            return Ok(None);
        }

        let next_is_string = matches!(
            self.peek_token(),
            Some(Token::DQuote | Token::SQuote | Token::BQuote)
        );
        let next_is_brace = matches!(self.peek_token(), Some(Token::LBrace));

        if next_is_brace && matches!(s, "m" | "n" | "nbt" | "minecraft_nbt") {
            self.bump();
            let raw = self.parse_snbt()?;
            return Ok(Some(self.arena.alloc(Expr::Nbt(NbtExpr {
                raw,
                span: start_span,
            }))));
        }

        if !next_is_string {
            return Ok(None);
        }

        if matches!(self.peek_token(), Some(Token::BQuote))
            && matches!(
                s,
                "line"
                    | "l"
                    | "local"
                    | "g"
                    | "game"
                    | "s"
                    | "save"
                    | "i"
                    | "inline"
                    | "j"
                    | "jmcc"
            )
        {
            self.bump();
            let scope = match s {
                "l" | "local" => VarScope::Local,
                "line" => VarScope::Line,
                "g" | "game" => VarScope::Game,
                "s" | "save" => VarScope::Save,
                "i" | "inline" => VarScope::Inline,
                "j" | "jmcc" => VarScope::Jmcc,
                _ => self.default_scope,
            };
            let tv = self.parse_string(TextParsing::Legacy)?;
            return Ok(Some(self.arena.alloc(Expr::Variable(VariableExpr {
                name: tv,
                scope,
                value_type: None,
                span: start_span,
            }))));
        }

        if matches!(
            s,
            "m" | "minimessage" | "p" | "plain" | "l" | "legacy" | "j" | "json"
        ) {
            self.bump();
            let parsing = match s {
                "p" | "plain" => TextParsing::Plain,
                "m" | "minimessage" => TextParsing::MiniMessage,
                "j" | "json" => TextParsing::Json,
                _ => TextParsing::Legacy,
            };
            let tv = self.parse_string(parsing)?;
            return Ok(Some(self.arena.alloc(Expr::Text(tv))));
        }

        Ok(None)
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_snbt(&mut self) -> Result<Spur> {
        let start_span = self.span();
        self.bump();
        let mut depth = 1;

        while depth > 0 {
            if self.is_eof() {
                return Err(JmccError::UnexpectedEof {
                    context: "SNBT block".into(),
                });
            }

            match self.curr_token() {
                Some(Token::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                Some(Token::RBrace) => {
                    depth -= 1;
                    if depth == 0 {
                        let end_span = self.span();
                        self.bump();
                        let content =
                            &self.input[start_span.end - self.offset..end_span.start - self.offset];
                        let mut s = String::with_capacity(content.len() + 2);
                        s.push('{');
                        s.push_str(content);
                        s.push('}');
                        return Ok(self.interner.get_or_intern(s));
                    }
                    self.bump();
                }
                Some(Token::DQuote | Token::SQuote | Token::BQuote) => {
                    self.bump();
                    while !matches!(self.curr(), LexedToken::StrEnd(_)) && !self.is_eof() {
                        self.bump();
                    }
                    if !self.is_eof() {
                        self.bump();
                    }
                }
                _ => {
                    self.bump();
                }
            }
        }
        unreachable!()
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_basic_literal(&mut self, start_span: Span) -> Result<ExprId> {
        let tok = self.bump();
        let expr = match &tok {
            LexedToken::Normal(Token::Number(s), _, _) => {
                let s = *s;
                let val = s
                    .replace('_', "")
                    .parse()
                    .map_err(|e| JmccError::NumberParse {
                        input: s.to_owned(),
                        source: e,
                    })?;
                Expr::Number(NumberExpr {
                    value: val,
                    span: start_span,
                })
            }
            LexedToken::Normal(Token::True, _, _) => Expr::Bool(BoolExpr {
                value: true,
                span: start_span,
            }),
            LexedToken::Normal(Token::False, _, _) => Expr::Bool(BoolExpr {
                value: false,
                span: start_span,
            }),
            LexedToken::Normal(Token::Ident(s), _, _) => {
                let name = self.interner.get_or_intern(*s);
                Expr::Ident(name, start_span)
            }
            LexedToken::Normal(
                Token::Line
                | Token::Local
                | Token::Game
                | Token::Save
                | Token::Inline
                | Token::Jmcc,
                span,
                _,
            ) => {
                let s = &self.input[span.clone()];
                let name = self.interner.get_or_intern(s);
                Expr::Ident(name, start_span)
            }
            LexedToken::Normal(Token::Return, _, _) if self.edition < 2026 => {
                let name = self.interner.get_or_intern("return");
                Expr::Ident(name, start_span)
            }
            _ => {
                return Err(JmccError::UnexpectedToken {
                    expected: "primary expression".into(),
                    got: format!("{tok:?}"),
                });
            }
        };
        Ok(self.arena.alloc(expr))
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_args(&mut self) -> Result<Vec<ArgExpr>> {
        let mut args = Vec::new();
        while !self.is_token(Token::RParen) && !self.is_eof() {
            let mut name = None;
            let mut spread = 0;
            let mut is_ref = false;

            let is_ident_like = match self.curr_token() {
                Some(Token::Ident(_)) => true,
                Some(_) => {
                    let s =
                        &self.input[self.span().start - self.offset..self.span().end - self.offset];
                    s.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                }
                None => false,
            };

            if is_ident_like && matches!(self.peek_token(), Some(Token::Assign)) {
                name = Some(self.parse_ident_str()?);
                self.bump();
            }

            if self.eat(Token::DoubleStar) {
                spread = 2;
            } else if self.eat(Token::Mul) {
                spread = 1;
            }
            if self.eat(Token::Ref) {
                is_ref = true;
            }

            let value = self.parse_expr(0)?;
            args.push(ArgExpr {
                name,
                value,
                spread,
                is_ref,
            });

            if !self.eat(Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn is_lambda_at_lparen(&mut self) -> bool {
        let mut depth = 0;
        let mut i = 0;
        while let Some(tok) = self.peek_token_at(i) {
            match tok {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        match self.peek_token_at(i + 1) {
                            Some(Token::FatArrow) => return true,
                            Some(Token::Arrow) => {
                                let mut j = i + 2;
                                while let Some(next_tok) = self.peek_token_at(j) {
                                    if next_tok == Token::FatArrow {
                                        return true;
                                    }
                                    if matches!(
                                        next_tok,
                                        Token::Semicolon | Token::LBrace | Token::RBrace
                                    ) {
                                        return false;
                                    }
                                    j += 1;
                                }
                                return false;
                            }
                            _ => return false,
                        }
                    }
                }
                Token::Semicolon => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_lambda_paren_expr(&mut self, start_span: Span) -> Result<ExprId> {
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        let return_type = self.parse_optional_type(Token::Arrow)?;
        self.expect(Token::FatArrow)?;
        let body = if self.is_token(Token::LBrace) {
            LambdaBody::Block(self.parse_block()?)
        } else {
            LambdaBody::Expr(self.parse_expr(0)?)
        };
        let end_span = self.last_span().end;
        Ok(self.arena.alloc(Expr::Lambda(LambdaExpr {
            params,
            return_type,
            body,
            span: start_span.start..end_span,
        })))
    }

    #[instrument(skip(self, start_span), level = "trace")]
    fn parse_lambda_single_ident_expr(&mut self, start_span: Span) -> Result<ExprId> {
        let name = self.parse_ident_str()?;
        let param_span = self.last_span();
        self.expect(Token::FatArrow)?;
        let param = Param {
            name,
            ty: None,
            default: None,
            is_ref: false,
            spread: 0,
            span: param_span,
        };
        let body = if self.is_token(Token::LBrace) {
            LambdaBody::Block(self.parse_block()?)
        } else {
            LambdaBody::Expr(self.parse_expr(0)?)
        };
        let end_span = self.last_span().end;
        Ok(self.arena.alloc(Expr::Lambda(LambdaExpr {
            params: vec![param],
            return_type: None,
            body,
            span: start_span.start..end_span,
        })))
    }
}
