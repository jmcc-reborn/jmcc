//! Selectors and game values: parsing MIR selector nodes and assembling
//! `Value::GameValue` alongside its `selection`.

use super::*;

impl CodeGen {
    pub(super) fn emit_game_value(
        e: &RecExpr<Mir>,
        [name, selector]: [Id; 2],
    ) -> CgResult<Value<'static>> {
        let name = extract_str(e, name)?;
        let game_value = serde_json::from_str(&format!("\"{name}\""))
            .map_err(|source| CodegenError::InvalidGameValue(name, source))?;
        let selection_type = match &e[selector] {
            Mir::Nop => "default".to_owned(),
            Mir::Sel(id) => extract_str(e, *id)?,
            other => return Err(CodegenError::InvalidSelector(other.clone())),
        };
        let selection = serde_json::to_string(&serde_json::json!({"type": selection_type}))?;
        Ok(Value::GameValue {
            game_value,
            selection: Cow::Owned(selection),
        })
    }
}
