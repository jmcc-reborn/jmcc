//! Управление потоком. Контейнеры и вызовы исполняет стек планировщика.
use crate::error::{ExceptionKind, Result, RuntimeError};
use crate::interp::{Args, Flow};
use crate::run::{LocalVariables, Runtime, Stream, spawn_stream};
use crate::scope::Shared;
use crate::value;
use jmcdata::generated::ActionId;
use jmcdata::module::{Op, Value, VariableScope};
use std::borrow::Cow;

#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    match op.action {
        ActionId::ControlEndThread => Ok(Flow::EndThread),
        ActionId::ControlReturnFunction => Ok(Flow::Return),
        ActionId::ControlStopRepeat => Ok(Flow::StopRepeat),
        ActionId::ControlSkipIteration => Ok(Flow::SkipIteration),
        ActionId::ControlCallException => {
            let args = Args::of(op);
            let id = rt.optional_arg(stream, args, "id")?;
            let message = rt.optional_arg(stream, args, "message")?;
            let kind = rt.optional_enum_arg(stream, args, "type")?;
            // Без `type` поднимается предупреждение: оно попадает в журнал, но
            // поток не останавливает (`tests/cubed` вызывает
            // `code::call_exception(message=...)` и продолжает работу).
            Err(RuntimeError::Raised {
                id: id.as_ref().map(value::display).filter(|id| !id.is_empty()),
                message: message.as_ref().map_or_else(String::new, value::display),
                kind: kind
                    .and_then(ExceptionKind::parse)
                    .unwrap_or(ExceptionKind::Warning),
            })
        }
        ActionId::StartProcess => start_process(rt, stream, op),
        ActionId::ControlDummy
        | ActionId::ControllerDoNotRun
        | ActionId::ControllerAsyncRun
        | ActionId::ControllerLabel
        | ActionId::ControllerIsolatedSelection => Ok(Flow::Continue),
        _other => rt.unimplemented_op(stream, op),
    }
}

/// Вычисляет задержку, не изменяя мировые часы.
///
/// Задержка — ровно то, что написано в `duration`, пересчитанное в тики по
/// `time_unit` (`TICKS` по умолчанию): ноль означает «не ждать», а не «ждать
/// один тик». Запасного значения здесь нет — единицы измерения заданы в самом
/// действии, и подменять `0 TICKS` на `1` значило бы врать программе о том,
/// когда продолжится её поток.
///
/// Дробная задержка округляется вверх: тик — неделимая единица.
#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "a finite non-negative duration is rounded up and saturates to u64"
)]
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub fn wait_ticks<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<u64> {
    let args = Args::of(op);
    let duration = rt
        .optional_number_arg(stream, args, "duration")?
        .unwrap_or(0.0);
    let ticks = duration
        * match rt.optional_enum_arg(stream, args, "time_unit")? {
            Some("SECONDS") => 20.0,
            Some("MINUTES") => 1200.0,
            _ => 1.0,
        };
    Ok(if ticks.is_finite() && ticks > 0.0 {
        ticks.ceil() as u64
    } else {
        0
    })
}

#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn start_process<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let name = rt.text_arg(stream, args, "process_name")?;
    let mode = match rt.optional_enum_arg(stream, args, "local_variables_mode")? {
        Some("COPY") => LocalVariables::Copy,
        Some("SHARE") => LocalVariables::Share,
        _ => LocalVariables::DontCopy,
    };
    let target_mode = rt.optional_enum_arg(stream, args, "target_mode")?;
    // Аргументы читаются здесь, в кадре вызывающего. Ссылок у процесса нет:
    // он живёт своей жизнью и переживёт этот кадр, поэтому `variable`-параметр
    // процесса получает значение, а не переменную вызывающего.
    let parameters = rt.resolve_arguments(stream, None, &[], parameters(args)?)?;
    let index = rt.program().process_index(&name)?;
    let groups = match target_mode {
        Some("CURRENT_SELECTION") => vec![stream.selection.clone()],
        Some("NO_TARGET") => vec![Vec::new()],
        Some("FOR_EACH_IN_SELECTION") => stream
            .selection
            .iter()
            .map(|target| vec![*target])
            .collect(),
        _ => vec![stream.targets.clone()],
    };
    for targets in groups {
        let mut frame = spawn_stream(targets, format!("process {name}"));
        frame.local = match mode {
            LocalVariables::DontCopy => Shared::new(),
            LocalVariables::Copy => Shared::with_vars(stream.local.snapshot()),
            LocalVariables::Share => stream.local.clone(),
        };
        rt.enqueue_handler(frame, index, parameters.clone(), None)?;
    }
    Ok(Flow::Continue)
}

/// Аргументы вызова как они записаны в операции, без вычисления.
///
/// Значение аргумента нельзя вычислять здесь: `line`-переменная в нём означает
/// переменную вызывающего, а кадр к этому моменту ещё не подменён. Кто и когда
/// их читает, решает [`Runtime::resolve_arguments`].
///
/// # Errors
///
/// Возвращает [`RuntimeError::UnknownParameter`], если ключ словаря `args` —
/// не текст с именем параметра.
#[tracing::instrument(level = "trace", skip(args))]
pub fn parameters<'a>(args: Args<'a>) -> Result<Vec<(String, &'a Value<'a>)>> {
    let Some(Value::Map { values }) = args.raw("args") else {
        return Ok(Vec::new());
    };
    let mut parameters = Vec::with_capacity(values.len());
    for (key, value) in values.iter() {
        let parsed: Option<serde_json::Value> = serde_json::from_str(&key.0).ok();
        let name = parsed
            .as_ref()
            .and_then(|v| v.get("text"))
            .and_then(serde_json::Value::as_str);
        let Some(name) = name else {
            return Err(RuntimeError::UnknownParameter {
                function: args
                    .raw("function_name")
                    .or_else(|| args.raw("process_name"))
                    .and_then(value::as_text)
                    .map_or_else(
                        || crate::schema::name(args.action()).to_owned(),
                        Cow::into_owned,
                    ),
                parameter: key.0.clone(),
            });
        };
        parameters.push((name.to_owned(), value));
    }
    Ok(parameters)
}

#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
pub fn optional_target<'a>(
    rt: &mut Runtime<'a>,
    stream: &Stream<'a>,
    args: Args<'a>,
    name: &'static str,
) -> Result<Option<(VariableScope, String)>> {
    args.raw(name).map_or(Ok(None), |value| {
        rt.variable_target(stream, value, args.action(), name)
            .map(Some)
    })
}
