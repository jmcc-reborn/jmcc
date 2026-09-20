//! Lowering HIR expressions into MIR values: literals, collections, wrappers,
//! arithmetic, comparisons, bitwise operations, indexing, and slices.

use super::*;

enum BinOpArgs {
    ValueArray,
    Remainder,
    Power,
}

enum CollectionKind {
    List,
    Map,
    Concat,
    Constructor,
}

enum WrapperKind {
    Enum,
    Nbt,
    Selector,
}

impl MirLowerer<'_> {
    #[instrument(skip(self), level = "trace")]
    pub(super) fn lower_expr(&mut self, id: Id) -> Result<Id, MirError> {
        let mut actual_id = id;
        while let Some(&next) = self.var_subst.get(&actual_id) {
            actual_id = next;
        }
        let hir_node = &self.hir[actual_id];
        trace!(?hir_node, "Lowering expr");

        let res = match hir_node {
            Hir::Num(n) => self.add(Mir::Num(*n)),
            Hir::Bool(b) => self.add(Mir::Bool(*b)),
            Hir::Str(s) => self.add(Mir::Str(s.clone())),
            Hir::Var(v) => self.add(Mir::Var(v.clone())),

            Hir::Local(value) => self.lower_scoped(*value, ScopeKind::Local)?,
            Hir::Game(value) => self.lower_scoped(*value, ScopeKind::Game)?,
            Hir::Save(value) => self.lower_scoped(*value, ScopeKind::Save)?,
            Hir::Line(value) => self.lower_scoped(*value, ScopeKind::Line)?,

            Hir::List(ids) => self.lower_collection(ids, CollectionKind::List)?,
            Hir::Map(ids) => self.lower_collection(ids, CollectionKind::Map)?,
            Hir::Concat(ids) => self.lower_collection(ids, CollectionKind::Concat)?,
            Hir::Named(ids) => self.lower_named(*ids)?,
            Hir::Enum(value) => self.lower_wrapped(*value, WrapperKind::Enum)?,
            Hir::Text(ids) => self.lower_text(*ids)?,
            Hir::Nbt(value) => self.lower_wrapped(*value, WrapperKind::Nbt)?,
            Hir::Sel(value) => self.lower_wrapped(*value, WrapperKind::Selector)?,

            Hir::FuncDecl(ids) => self.lower_function_decl(*ids, FunctionKind::Function)?,
            Hir::ProcDecl(ids) => self.lower_function_decl(*ids, FunctionKind::Process)?,
            Hir::EventDecl(ids) => self.lower_event_decl(*ids)?,
            Hir::ClassDecl([_, _, body]) => self.lower_class_decl(*body)?,
            Hir::EnumDecl(_) | Hir::Nop => self.nop(),

            Hir::FuncCall([target, args]) => self.lower_func_call(*target, *args)?,

            Hir::ProcCall([target, args]) => self.lower_proc_call(*target, *args)?,

            Hir::Ctor(ids) => self.lower_collection(ids, CollectionKind::Constructor)?,

            Hir::Action(ids) => self.lower_action(ids, false)?,

            Hir::Let(ids) => self.lower_let(*ids)?,

            Hir::Not(a) => {
                let inner = self.lower_expr(*a)?;
                self.add(Mir::Not(inner))
            }

            Hir::Add([a, b]) => self.lower_arith(*a, *b, "add", BinOpArgs::ValueArray)?,
            Hir::Sub([a, b]) => self.lower_arith(*a, *b, "subtract", BinOpArgs::ValueArray)?,
            Hir::Mul([a, b]) => self.lower_arith(*a, *b, "multiply", BinOpArgs::ValueArray)?,
            Hir::Div([a, b]) => self.lower_arith(*a, *b, "divide", BinOpArgs::ValueArray)?,
            Hir::Mod([a, b]) => self.lower_arith(*a, *b, "remainder", BinOpArgs::Remainder)?,
            Hir::Pow([a, b]) => self.lower_arith(*a, *b, "pow", BinOpArgs::Power)?,

            Hir::Eq([a, b]) => self.lower_cmp(*a, *b, "equals")?,
            Hir::Ne([a, b]) => self.lower_cmp(*a, *b, "not_equals")?,
            Hir::Lt([a, b]) => self.lower_cmp(*a, *b, "less")?,
            Hir::Le([a, b]) => self.lower_cmp(*a, *b, "less_or_equals")?,
            Hir::Gt([a, b]) => self.lower_cmp(*a, *b, "greater")?,
            Hir::Ge([a, b]) => self.lower_cmp(*a, *b, "greater_or_equals")?,

            Hir::And([a, b]) | Hir::BitAnd([a, b]) => self.lower_bitwise(*a, *b, "AND")?,
            Hir::Or([a, b]) | Hir::BitOr([a, b]) => self.lower_bitwise(*a, *b, "OR")?,
            Hir::BitXor([a, b]) => self.lower_bitwise(*a, *b, "XOR")?,
            Hir::Shl([a, b]) => self.lower_bitwise(*a, *b, "LEFT_SHIFT")?,
            Hir::Shr([a, b]) => self.lower_bitwise(*a, *b, "RIGHT_SHIFT")?,

            Hir::Neg(value) => self.lower_neg(*value)?,
            Hir::Inc(value) => self.lower_update(*value, "add")?,
            Hir::Dec(value) => self.lower_update(*value, "subtract")?,

            Hir::If(ids) => self.lower_if_expr(*ids)?,
            Hir::While(ids) => self.lower_while_expr(*ids)?,

            Hir::Break => self.add(Mir::Break),

            Hir::Return(value) => self.lower_return(*value)?,

            Hir::VarDecl(ids) => self.lower_var_decl(*ids)?,

            Hir::Set([target, val]) => self.lower_set(*target, *val)?,

            Hir::Block(ids) => self.lower_block_expr(ids)?,

            Hir::Index(ids) => self.lower_index(*ids)?,
            Hir::Slice(ids) => self.lower_slice(*ids)?,
        };
        Ok(res)
    }

    fn lower_collection(&mut self, ids: &[Id], kind: CollectionKind) -> Result<Id, MirError> {
        let ids = ids
            .iter()
            .map(|id| self.lower_expr(*id))
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        Ok(match kind {
            CollectionKind::List => self.add(Mir::List(ids)),
            CollectionKind::Map => self.add(Mir::Map(ids)),
            CollectionKind::Concat => self.add(Mir::Concat(ids)),
            CollectionKind::Constructor => self.add(Mir::Ctor(ids)),
        })
    }

    fn lower_named(&mut self, [name, value]: [Id; 2]) -> Result<Id, MirError> {
        let name = self.lower_expr(name)?;
        let value = self.lower_expr(value)?;
        Ok(self.add(Mir::Named([name, value])))
    }

    fn lower_wrapped(&mut self, value: Id, kind: WrapperKind) -> Result<Id, MirError> {
        let value = self.lower_expr(value)?;
        Ok(match kind {
            WrapperKind::Enum => self.add(Mir::Enum(value)),
            WrapperKind::Nbt => self.add(Mir::Nbt(value)),
            WrapperKind::Selector => self.add(Mir::Sel(value)),
        })
    }

    fn lower_text(&mut self, [text_type, content]: [Id; 2]) -> Result<Id, MirError> {
        let text_type = self.lower_expr(text_type)?;
        let content = self.lower_expr(content)?;
        Ok(self.add(Mir::Text([text_type, content])))
    }

    fn lower_neg(&mut self, value: Id) -> Result<Id, MirError> {
        let value = self.lower_expr(value)?;
        let temp = self.fresh_temp();
        let minus_one = self.add(Mir::Num((-1.0).into()));
        let values = self.add(Mir::List(vec![value, minus_one].into_boxed_slice()));
        self.lower_arith_op(temp, values, "multiply")
    }

    fn lower_update(&mut self, value: Id, operation: &str) -> Result<Id, MirError> {
        let value = self.lower_expr(value)?;
        let temp = self.fresh_temp();
        let one = self.add(Mir::Num(1.0.into()));
        let values = self.add(Mir::List(vec![value, one].into_boxed_slice()));
        let result = self.lower_arith_op(temp, values, operation)?;
        let set_back = self.make_set_var(value, temp);
        Ok(self.add(Mir::Block(vec![result, set_back, temp].into_boxed_slice())))
    }

    fn lower_index(&mut self, [object, index]: [Id; 2]) -> Result<Id, MirError> {
        let object = self.lower_expr(object)?;
        let index = self.lower_expr(index)?;
        let temp = self.fresh_temp();
        let default = self.add(Mir::Num(0.0.into()));
        let args = vec![
            self.named_arg("variable", temp),
            self.named_arg("list", object),
            self.named_arg("index", index),
            self.named_arg("default_value", default),
        ];
        let action = self.make_action("variable", "get_list_value", args);
        Ok(self.add(Mir::Let([temp, action, temp])))
    }

    fn lower_slice(&mut self, [object, start, end]: [Id; 3]) -> Result<Id, MirError> {
        let object = self.lower_expr(object)?;
        let start = self.lower_expr(start)?;
        let end = self.lower_expr(end)?;
        let temp = self.fresh_temp();
        let args = vec![
            self.named_arg("variable", temp),
            self.named_arg("list", object),
            self.named_arg("start_index", start),
            self.named_arg("end_index", end),
        ];
        let action = self.make_action("variable", "trim_list", args);
        Ok(self.add(Mir::Let([temp, action, temp])))
    }

    #[instrument(skip(self, args_kind), level = "trace")]
    fn lower_arith(
        &mut self,
        left_hir: Id,
        right_hir: Id,
        action_name: &str,
        args_kind: BinOpArgs,
    ) -> Result<Id, MirError> {
        let left = self.lower_expr(left_hir)?;
        let right = self.lower_expr(right_hir)?;
        let temp = self.fresh_temp();

        let assign_arg = self.named_arg("variable", temp);
        let mut args = vec![assign_arg];

        match args_kind {
            BinOpArgs::ValueArray => {
                let list = self.add(Mir::List(vec![left, right].into_boxed_slice()));
                args.push(self.named_arg("value", list));
            }
            BinOpArgs::Remainder => {
                args.push(self.named_arg("dividend", left));
                args.push(self.named_arg("divisor", right));
                let mode_str = self.str_id("MODULO");
                let mode_val = self.add(Mir::Enum(mode_str));
                args.push(self.named_arg("remainder_mode", mode_val));
            }
            BinOpArgs::Power => {
                args.push(self.named_arg("base", left));
                args.push(self.named_arg("power", right));
            }
        }

        let action = self.make_action("variable", action_name, args);
        Ok(self.add(Mir::Let([temp, action, temp])))
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_arith_op(
        &mut self,
        temp: Id,
        val_args_id: Id,
        action_name: &str,
    ) -> Result<Id, MirError> {
        let args = vec![
            self.named_arg("variable", temp),
            self.named_arg("value", val_args_id),
        ];
        let action = self.make_action("variable", action_name, args);
        Ok(self.add(Mir::Let([temp, action, temp])))
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_cmp(
        &mut self,
        left_hir: Id,
        right_hir: Id,
        action_name: &str,
    ) -> Result<Id, MirError> {
        let left = self.lower_expr(left_hir)?;
        let right = self.lower_expr(right_hir)?;
        let temp = self.fresh_temp();

        let true_val = self.add(Mir::Num(1.0.into()));
        let set_true = self.make_set_var(temp, true_val);
        let then_block = self.add(Mir::Block(vec![set_true].into_boxed_slice()));

        let zero_val = self.add(Mir::Num(0.0.into()));
        let set_zero = self.make_set_var(temp, zero_val);
        let else_block = self.add(Mir::Block(vec![set_zero].into_boxed_slice()));

        let if_action = self.cmp_action(action_name, left, right, then_block);
        let else_act = self.else_action(else_block);

        // The final node determines the block's value in code generation.
        let block = self.add(Mir::Block(
            vec![if_action, else_act, temp].into_boxed_slice(),
        ));
        Ok(self.add(Mir::Let([temp, block, temp])))
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_bitwise(
        &mut self,
        left_hir: Id,
        right_hir: Id,
        operator: &str,
    ) -> Result<Id, MirError> {
        let left = self.lower_expr(left_hir)?;
        let right = self.lower_expr(right_hir)?;
        let temp = self.fresh_temp();

        let op_str = self.str_id(operator);
        let op_val = self.add(Mir::Enum(op_str));

        let args = vec![
            self.named_arg("variable", temp),
            self.named_arg("operand1", left),
            self.named_arg("operand2", right),
            self.named_arg("operator", op_val),
        ];

        let action = self.make_action("variable", "bitwise_operation", args);
        Ok(self.add(Mir::Let([temp, action, temp])))
    }
}
