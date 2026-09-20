//! Variable lowering and scoping: Line/Local/Game/Save wrappers,
//! variable declarations, value writes, and assignments.

use super::*;

impl MirLowerer<'_> {
    #[instrument(skip(self), level = "trace")]
    pub(super) fn make_set_var(&mut self, var: Id, val: Id) -> Id {
        let val_node = &self.nodes.as_slice()[usize::from(val)];
        match val_node {
            Mir::List(_) => {
                let args = vec![
                    self.named_arg("variable", var),
                    self.named_arg("values", val),
                ];
                self.make_action("variable", "create_list", args)
            }
            Mir::Map(ids) => {
                let mut keys = Vec::new();
                let mut values = Vec::new();

                for chunk in ids.chunks(2) {
                    if chunk.len() == 2 {
                        keys.push(chunk[0]);
                        values.push(chunk[1]);
                    }
                }

                let keys_list = self.add(Mir::List(keys.into_boxed_slice()));
                let values_list = self.add(Mir::List(values.into_boxed_slice()));

                let args = vec![
                    self.named_arg("variable", var),
                    self.named_arg("keys", keys_list),
                    self.named_arg("values", values_list),
                ];
                self.make_action("variable", "create_map_from_values", args)
            }
            _ => {
                let args = vec![
                    self.named_arg("variable", var),
                    self.named_arg("value", val),
                ];
                self.make_action("variable", "set_value", args)
            }
        }
    }

    pub(super) fn lower_scoped(&mut self, value: Id, scope: ScopeKind) -> Result<Id, MirError> {
        let value = self.lower_expr(value)?;
        Ok(match scope {
            ScopeKind::Local => self.add(Mir::Local(value)),
            ScopeKind::Game => self.add(Mir::Game(value)),
            ScopeKind::Save => self.add(Mir::Save(value)),
            ScopeKind::Line => self.add(Mir::Line(value)),
        })
    }

    pub(super) fn lower_var_decl(&mut self, [name, value]: [Id; 2]) -> Result<Id, MirError> {
        let name = self.lower_expr(name)?;
        let value = self.lower_expr(value)?;
        if matches!(self.nodes.as_slice()[usize::from(value)], Mir::Nop) {
            return Ok(self.nop());
        }
        if let Some(bound) = self.bind_in_place_target(value, name) {
            return Ok(bound);
        }
        Ok(self.make_set_var(name, value))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn lower_set(&mut self, target: Id, val: Id) -> Result<Id, MirError> {
        match &self.hir[target] {
            Hir::Slice([_obj, _start, _end]) => Err(MirError::SliceAssignmentNotSupported),
            Hir::Index([obj, idx]) => {
                let obj_val = self.lower_expr(*obj)?;
                let idx_val = self.lower_expr(*idx)?;
                let val_id = self.lower_expr(val)?;

                // `set_list_value` needs the same value as both source and destination.
                let args = vec![
                    self.named_arg("list", obj_val),
                    self.named_arg("number", idx_val),
                    self.named_arg("value", val_id),
                    self.named_arg("variable", obj_val),
                ];

                Ok(self.make_action("variable", "set_list_value", args))
            }
            _ => {
                let target_id = self.lower_expr(target)?;
                let val_id = self.lower_expr(val)?;
                if let Some(bound) = self.bind_in_place_target(val_id, target_id) {
                    return Ok(bound);
                }
                Ok(self.make_set_var(target_id, val_id))
            }
        }
    }

    fn same_var_symbol(&self, a: Id, b: Id) -> bool {
        let mut curr_a = a;
        while let Mir::Line(inner) | Mir::Local(inner) | Mir::Game(inner) | Mir::Save(inner) =
            &self.nodes.as_slice()[usize::from(curr_a)]
        {
            curr_a = *inner;
        }
        let mut curr_b = b;
        while let Mir::Line(inner) | Mir::Local(inner) | Mir::Game(inner) | Mir::Save(inner) =
            &self.nodes.as_slice()[usize::from(curr_b)]
        {
            curr_b = *inner;
        }
        match (
            &self.nodes.as_slice()[usize::from(curr_a)],
            &self.nodes.as_slice()[usize::from(curr_b)],
        ) {
            (Mir::Var(v1), Mir::Var(v2)) => v1.0 == v2.0,
            _ => a == b,
        }
    }

    /// Binds the assignment target to the `variable` slot of an in-place action,
    /// avoiding an intermediate temporary and redundant `set_value`.
    fn bind_in_place_target(&mut self, value_id: Id, target: Id) -> Option<Id> {
        let (act_node_id, temp) = match &self.nodes.as_slice()[usize::from(value_id)] {
            Mir::Let([temp, act_id, body]) if *temp == *body => (*act_id, *temp),
            _ => return None,
        };

        let Mir::Action(ids) = self.nodes.as_slice()[usize::from(act_node_id)].clone() else {
            return None;
        };
        let Mir::List(args) = self.nodes.as_slice()[usize::from(ids[3])].clone() else {
            return None;
        };

        let mut new_args = args.to_vec();
        let mut replaced = false;

        for arg in &mut new_args {
            let Mir::Named([name_id, val]) = self.nodes.as_slice()[usize::from(*arg)] else {
                continue;
            };
            if self.same_var_symbol(val, temp) {
                *arg = self.add(Mir::Named([name_id, target]));
                replaced = true;
                break;
            }
        }

        if !replaced {
            return None;
        }

        let new_args_list = self.add(Mir::List(new_args.into_boxed_slice()));
        let mut new_ids = ids.to_vec();
        new_ids[3] = new_args_list;
        Some(self.add(Mir::Action(new_ids.into_boxed_slice())))
    }
}
