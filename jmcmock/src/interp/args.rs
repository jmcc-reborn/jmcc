//! Аргументы операций: доступ к значениям, цель записи и ошибки аргументов.

use super::*;

impl<'a> Runtime<'a> {
    /// A variable name after placeholder substitution.
    ///
    /// A variable name in `JustMC` is text too: the compiler produces
    /// `game var %player%_dialog`, that is, one variable per player rather than a
    /// literal `%` in the name. Without substitution `player_join` would write to
    /// one variable while `start_process` read from another.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UndefinedVariable`] if the name's placeholder
    /// refers to a nonexistent variable.
    #[tracing::instrument(level = "trace", skip(self, stream), fields(raw = %raw))]
    pub(crate) fn variable_name(&mut self, stream: &Stream<'a>, raw: &str) -> Result<String> {
        if raw.contains("%math(") {
            self.unimplemented("the %math() expression")?;
            return Ok(raw.to_owned());
        }
        crate::text::substitute_placeholders(self, stream, raw)
    }

    /// The variable a write targets: an action with a variable argument.
    ///
    /// Returns the scope and the name, already with placeholders substituted —
    /// [`Self::scope_store`] uses them to find the store. The argument is not
    /// evaluated: the value behind a variable reference is never read, otherwise
    /// the write would overwrite what it had just read.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::MissingArgument`] if the argument is absent, and
    /// [`RuntimeError::ExpectedVariable`] if it is not a variable.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn target_of(
        &mut self,
        stream: &Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<(VariableScope, String)> {
        let Some(value) = args.values.get(name) else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action),
                arg: name,
            });
        };
        self.variable_target(stream, value, args.action, name)
    }

    /// The variable a write targets, from an already found argument value.
    ///
    /// Split off from [`Self::target_of`] for actions that address not an
    /// operation's argument but an element inside an argument:
    /// `set_variable_multiple` takes the variable name from each element of the
    /// `variables` array, and `args.values.get` would find nothing there.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::ExpectedVariable`] if the value is not a variable.
    #[tracing::instrument(level = "trace", skip(self, stream), fields(value = ?value, action = ?action, arg = %arg))]
    pub(crate) fn variable_target(
        &mut self,
        stream: &Stream<'a>,
        value: &Value<'a>,
        action: ActionId,
        arg: &'static str,
    ) -> Result<(VariableScope, String)> {
        let Value::Variable { variable, scope } = value else {
            return Err(RuntimeError::ExpectedVariable {
                action: schema::name(action),
                arg,
                actual: value::kind(value),
            });
        };
        let resolved = self.variable_name(stream, variable)?;
        Ok((*scope, resolved))
    }
}

impl<'a> Runtime<'a> {
    /// The value of an argument the operation must have.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::MissingArgument`] if the argument is absent.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Rt<'a>> {
        let Some(value) = args.values.get(name) else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action),
                arg: name,
            });
        };
        self.eval(stream, value)
    }

    /// The value of an argument the compiler may not have written.
    ///
    /// # Errors
    ///
    /// Returns an error evaluating the value if the argument is present and does
    /// not evaluate.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Option<Rt<'a>>> {
        args.values
            .get(name)
            .map_or(Ok(None), |value| self.eval(stream, value).map(Some))
    }

    /// A numeric argument.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotANumber`] if the value is not a number.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn number_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<f64> {
        let value = self.arg(stream, args, name)?;
        value::number_of(&value, &at_op(args, name))
    }

    /// A numeric argument that may be absent.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotANumber`] if the value is not a number.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_number_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Option<f64>> {
        match self.arg(stream, args, name) {
            Ok(value) => Ok(Some(value::number_of(&value, &at_op(args, name))?)),
            Err(RuntimeError::MissingArgument { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Numeric arguments: a single number or a list of numbers.
    ///
    /// The schema marks such arguments as `array`, but the compiler writes a
    /// single value into them too — arithmetic on one number looks like a number
    /// in JSON. A list is unfolded, a single value becomes a list of one element.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotANumber`] on a non-numeric element.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn numbers_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<f64>> {
        let mut numbers = Vec::new();
        for value in self.plural_values_arg(stream, args, name)? {
            numbers.push(value::number_of(&value, &at_op(args, name))?);
        }
        Ok(numbers)
    }

    /// A plural argument whose elements may be lists themselves.
    ///
    /// In a slot that means "several values" a list gives its elements, no
    /// matter how it got there: written as a literal, or computed earlier and
    /// held in a variable (`variable::min(variable::append_list([a, b]))`).
    /// [`Self::values_arg`] unfolds only literals, which is what arguments that
    /// take a single value need — there a list variable *is* the value, as in
    /// list comparison.
    ///
    /// # Errors
    ///
    /// Returns an error evaluating the value.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn plural_values_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<Rt<'a>>> {
        let mut result = Vec::new();
        for item in self.values_arg(stream, args, name)? {
            match item {
                Some(Value::Array { values }) => result.extend(values),
                other => result.push(other),
            }
        }
        Ok(result)
    }

    /// A plural argument the compiler may not have written.
    ///
    /// The schema has slots that are alternatives — `variable::remove_map_entry`
    /// takes a single `key` or a list of `values`, and a call gives one of them.
    /// An absent slot is an empty list, the same as [`Self::values_arg`] gives an
    /// empty one for an empty list.
    ///
    /// # Errors
    ///
    /// Returns an error evaluating the value if the argument is present and does
    /// not evaluate.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_values_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<Rt<'a>>> {
        match self.values_arg(stream, args, name) {
            Ok(values) => Ok(values),
            Err(RuntimeError::MissingArgument { .. }) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// A text argument.
    ///
    /// An action's text argument accepts any value: `JustMC` converts to text
    /// whatever landed in it (`value::argument_text`).
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnexpectedValue`] if the value has no text form at
    /// all.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn text_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Cow<'a, str>> {
        let value = self.arg(stream, args, name)?;
        value::text_of(&value, &at_op(args, name))
    }

    /// An enumeration argument: the canonical spelling from the schema is
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::InvalidEnumArgument`] if the value is not in the
    /// set from the schema.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn enum_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<&'static str> {
        let value = self.arg(stream, args, name)?;
        let allowed = schema::argument_values(args.action, name).unwrap_or(&[]);
        let Some(value) = value else {
            return Err(empty_enum(args, name));
        };
        value::enum_argument(&value, schema::name(args.action), name, allowed)
    }

    /// An enumeration argument that may be absent.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::InvalidEnumArgument`] if the value is not in the
    /// set from the schema.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_enum_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Option<&'static str>> {
        match self.enum_arg(stream, args, name) {
            Ok(value) => Ok(Some(value)),
            Err(RuntimeError::MissingArgument { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// A list of values.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotAList`] if the value is not a list.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn list_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<Rt<'a>>> {
        let value = self.arg(stream, args, name)?;
        value::list_of(&value, &at_op(args, name))
    }

    /// A list of values the compiler may not have written.
    ///
    /// The schema marks the slots of `variable::create_map` as `array`, but the
    /// game fills in two empty lists when a call gives neither — `variable::create_map()`
    /// is a valid way to write "an empty dictionary". An absent slot is an empty
    /// list here, the same as an explicitly empty one.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotAList`] if the argument is present and is not
    /// a list.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_list_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<Rt<'a>>> {
        match self.list_arg(stream, args, name) {
            Ok(values) => Ok(values),
            Err(RuntimeError::MissingArgument { .. }) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// A dictionary.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotAMap`] if the value is not a dictionary.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn map_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<LiteMap<TextValue, Value<'a>>> {
        let value = self.arg(stream, args, name)?;
        value::map_of(&value, &at_op(args, name))
    }

    /// A location: `[x, y, z, yaw, pitch]`.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotALocation`] if the value is not a location.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn location_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<[f64; 5]> {
        let value = self.arg(stream, args, name)?;
        value::location_of(&value, &at_op(args, name))
    }

    /// A location argument that may be absent or empty.
    ///
    /// Used where an absent location is an answer rather than a mistake: the
    /// value an action checks may be about a target the mock does not have (the
    /// victim of a fight), and then there is no range to be in.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotALocation`] if the argument is there and holds
    /// something that is not a location.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn optional_location_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Option<[f64; 5]>> {
        match self.optional_arg(stream, args, name)?.flatten() {
            None | Some(Value::Error) => Ok(None),
            Some(value) => value::location_of(&Some(value), &at_op(args, name)).map(Some),
        }
    }

    /// A vector: `[x, y, z]`.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotAVector`] if the value is not a vector.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn vector_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<[f64; 3]> {
        let value = self.arg(stream, args, name)?;
        value::vector_of(&value, &at_op(args, name))
    }

    /// A vector argument whose empty value means `[0, 0, 0]`.
    ///
    /// `JustMC` reads a variable that holds nothing as zero, and three zeroes is
    /// what that means for a vector slot. An action that changes one component of
    /// a vector has to start from something, and the program gets an empty vector
    /// whenever the value it works on is about a target the mock does not have.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NotAVector`] if the argument is there and holds
    /// something that is not a vector.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn vector_or_zero(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<[f64; 3]> {
        match self.optional_arg(stream, args, name)?.flatten() {
            None | Some(Value::Error) => Ok([0.0; 3]),
            Some(value) => value::vector_of(&Some(value), &at_op(args, name)),
        }
    }

    /// An argument with several values: a list or a single value.
    ///
    /// The schema marks such arguments as `array` (`compare: any[21]`,
    /// `value: number[21]`, `messages: text[18]`), but the compiler writes a
    /// single value into them too. The flag comes from the schema, the shape from
    /// JSON: a list is unfolded element by element, a single value becomes a list
    /// of one element.
    ///
    /// # Errors
    ///
    /// Returns an error evaluating the value.
    #[tracing::instrument(level = "trace", skip(self, stream, args), fields(name = %name))]
    pub(crate) fn values_arg(
        &mut self,
        stream: &mut Stream<'a>,
        args: Args<'a>,
        name: &'static str,
    ) -> Result<Vec<Rt<'a>>> {
        // The schema calls such arguments `array`, but in JSON even something the
        // schema does not count as a list may turn out to be one. Any literal
        // list is unfolded: in these arguments it always means "several values"
        // rather than "a list value" — a list value arrives as a variable
        // reference and stays a single element.
        if schema::argument_is_array(args.action, name)
            && let Some(Value::Array { values }) = args.values.get(name)
        {
            let mut items = Vec::with_capacity(values.len());
            for item in values {
                items.push(match item {
                    Some(item) => self.eval(stream, item)?,
                    None => None,
                });
            }
            return Ok(items);
        }
        Ok(vec![self.arg(stream, args, name)?])
    }
}

/// The place of a value in an error message: the action and the argument.
#[tracing::instrument(level = "trace", skip(args), fields(arg = %arg))]
pub fn at_op(args: Args<'_>, arg: &str) -> String {
    format!("action '{}', argument '{arg}'", schema::name(args.action))
}

/// The error about a value that is not in an enumeration argument's set.
///
/// The allowed values are looked up in the schema: the message is the only place
/// a program's author sees them.
#[tracing::instrument(level = "trace", skip(args), fields(arg = %arg, value = %value))]
pub fn invalid_enum(args: Args<'_>, arg: &'static str, value: String) -> RuntimeError {
    RuntimeError::InvalidEnumArgument {
        action: schema::name(args.action),
        arg,
        value,
        allowed: schema::argument_values(args.action, arg)
            .unwrap_or(&[])
            .join(", "),
    }
}

/// The error about an empty value in an enumeration argument.
///
/// An empty variable fits no value of the enumeration, and saying so should look
/// the same as for any unfitting value — by listing the allowed ones.
#[tracing::instrument(level = "trace", skip(args), fields(arg = %arg))]
fn empty_enum(args: Args<'_>, arg: &'static str) -> RuntimeError {
    invalid_enum(args, arg, "an empty variable".to_owned())
}
