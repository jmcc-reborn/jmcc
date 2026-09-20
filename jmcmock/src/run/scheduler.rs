//! Однопоточный кооперативный планировщик и явный стек продолжений.
//!
//! `JustMC` запускает каждый обработчик события и каждый процесс отдельным
//! потоком: они выполняются одновременно, а `wait` усыпляет только свой поток.
//! Мок повторяет это без потоков ОС. Поток — это [`Task`] с явным стеком
//! продолжений, а [`Scheduler`] по очереди возобновляет готовые задачи и
//! двигает виртуальные часы мира, когда все спят.
//!
//! # Почему явный стек, а не рекурсия
//!
//! `wait` может стоять где угодно — внутри ветвления, цикла, вложенного вызова
//! функции. Рекурсивный обход дерева операций приостановить нельзя, не сохранив
//! стек Rust целиком, поэтому «что делать, когда операция завершится» хранится
//! здесь явно, в [`Continuation`]. Обработка одной операции разбита на
//! [`Runtime::schedule_op`] — она только *планирует* продолжения — и сам цикл
//! [`Runtime::resume_task`], который их разбирает и применяет сигналы
//! (`return`, `break`, `skip`, конец потока).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use jmcdata::generated::ActionId;
use jmcdata::module::VariableScope;

use crate::actions::{code, repeat};
use crate::error::ExceptionKind;
use crate::interp::{Args, body_of, is_branch};
use crate::{schema, value};

use super::*;

/// Куда задача, запущенная извне планировщика, кладёт свой результат.
///
/// Внешний `None` — задача ещё не завершилась; `Some(value)` — завершилась, и
/// `value` — то, что осталось в `local:ret`. Внешний `Some` отличается от
/// внутреннего: пустая функция возвращает `Some(None)`, а не «ещё работает».
pub type Completion<'a> = Rc<RefCell<Option<Rt<'a>>>>;

/// Очереди задач: готовые к исполнению и спящие до виртуального тика.
#[derive(Debug, Default)]
pub(super) struct Scheduler<'a> {
    ready: VecDeque<Task<'a>>,
    sleeping: BTreeMap<u64, VecDeque<Task<'a>>>,
}

/// Один поток: кадр исполнения и стек продолжений.
#[derive(Debug)]
struct Task<'a> {
    stream: Stream<'a>,
    stack: Vec<Continuation<'a>>,
    /// Заполняется, только если задачу запустили ради её результата.
    completion: Option<Completion<'a>>,
}

/// Что делать, когда текущая операция завершится.
///
/// Стек читается сверху вниз: последний элемент — самая вложенная работа.
#[derive(Debug)]
enum Continuation<'a> {
    /// Блок операций: исполнить `ops[next]`, затем `ops[next + 1]`.
    /// `condition` — результат последнего `if_*` в этом блоке: его читает
    /// `else`, который компилятор пишет следующей операцией, а не вложенным
    /// блоком.
    Block {
        ops: &'a [Op<'a>],
        next: usize,
        condition: Option<bool>,
    },
    /// Место операции в модуле — обёртка ошибки, возникающей внутри неё.
    Location(String),
    /// Вернуть прежние цели кадра после операции со своим выделением.
    Targets(Vec<Target>),
    /// Вернуться в кадр вызывающего после завершения тела функции.
    Call {
        line: Shared<'a>,
        origin: String,
        callee: String,
    },
    /// Тело повтора: после него решить, будет ли следующая итерация.
    Loop {
        op: &'a Op<'a>,
        state: repeat::LoopState<'a>,
    },
    /// `controller_measure_time`: записать прошедшие тики.
    Measure {
        scope: VariableScope,
        name: String,
        start: u64,
        scale: f64,
    },
    /// `controller_exception`: перехватить ошибку тела.
    Catch {
        scope: VariableScope,
        name: String,
        kind: ExceptionKind,
    },
    /// `controller_label`: метка для `control_break_label`.
    Label { label: String },
}

/// Что операция сообщила планировщику сразу после запуска.
enum Step {
    /// Продолжить со следующей операции блока.
    Continue,
    /// Задача уснула на столько тиков.
    Sleep(u64),
    /// Сигнал наверх: `return`, `break`, `skip` или конец потока.
    Signal(Flow),
}

impl<'a> Task<'a> {
    /// Кладёт в стек блок операций, который надо исполнить.
    #[tracing::instrument(level = "debug", skip(self, ops))]
    fn block(&mut self, ops: &'a [Op<'a>]) {
        self.stack.push(Continuation::Block {
            ops,
            next: 0,
            condition: None,
        });
    }
}

impl<'a> Runtime<'a> {
    /// Настраивает кадр под обработчик: объявляет строчные переменные и
    /// параметры, кладёт аргументы и обнуляет `ret`.
    ///
    /// `variable`-параметры не кладутся значением: для них ставится ссылка на
    /// переменную вызывающего (см. [`Bound::Reference`]), и тогда запись в
    /// параметр внутри функции меняет переменную снаружи.
    ///
    /// # Errors
    ///
    /// Возвращает [`RuntimeError::MissingParameter`], если вызов не передал
    /// значение объявленного параметра.
    #[tracing::instrument(level = "debug", skip(self, stream), fields(index = ?index, args = ?args))]
    fn bind_handler(
        &self,
        stream: &Stream<'a>,
        index: usize,
        args: Vec<(String, Bound<'a>)>,
    ) -> Result<()> {
        let declared = self.program.parameters(index).to_vec();
        for param in &declared {
            if !param.by_ref && !args.iter().any(|(supplied, _)| supplied == &param.name) {
                return Err(RuntimeError::MissingParameter {
                    function: self.program.name(index),
                    parameter: param.name.clone(),
                });
            }
        }
        // Ссылки ставятся до объявления строчных переменных: иначе `declare`
        // создал бы у вызываемого собственную пустую переменную с тем же именем.
        for (name, bound) in &args {
            if let Bound::Reference(store, target) = bound {
                stream.line.link(name, store.clone(), target);
            }
        }
        for name in self
            .program
            .line_vars(index)
            .iter()
            .chain(declared.iter().map(|param| &param.name))
        {
            stream.line.declare(name);
        }
        stream.local.set(RETURN_VARIABLE, None);
        for (name, bound) in args {
            if let Bound::Value(value) = bound {
                stream.line.set(&name, value);
            }
        }
        Ok(())
    }

    /// Превращает аргументы вызова в то, что получит кадр вызываемого.
    ///
    /// Читает значения **в кадре вызывающего**: аргумент вида
    /// `{"type":"variable","variable":"x","scope":"line"}` должен означать
    /// переменную вызывающего, а не вызываемого. По той же причине здесь же
    /// разбираются ссылки.
    ///
    /// `caller_line` — строчное хранилище вызывающего; `None` означает «кадр
    /// не вызывает, а запускает процесс»: тогда ссылок не ставится, потому что
    /// процесс живёт своей жизнью и не должен писать в чужой кадр.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку вычисления значения аргумента и ошибку имени
    /// переменной-ссылки.
    #[tracing::instrument(level = "debug", skip(self, stream, declared, args), fields(count = ?args.len()))]
    pub(crate) fn resolve_arguments(
        &mut self,
        stream: &mut Stream<'a>,
        caller_line: Option<&Shared<'a>>,
        declared: &[Param],
        args: Vec<(String, &'a Value<'a>)>,
    ) -> Result<Vec<(String, Bound<'a>)>> {
        let mut bound = Vec::with_capacity(args.len());
        for (name, raw) in args {
            let by_ref = declared
                .iter()
                .any(|param| param.name == name && param.by_ref);
            if by_ref
                && let Some(caller_line) = caller_line
                && let Value::Variable { variable, scope } = raw
            {
                let target = self.variable_name(stream, variable)?;
                let store = match scope {
                    VariableScope::Line => caller_line.clone(),
                    other => self.scope_store(stream, *other),
                };
                bound.push((name, Bound::Reference(store, target)));
                continue;
            }
            bound.push((name, Bound::Value(self.eval(stream, raw)?)));
        }
        Ok(bound)
    }

    /// Ставит обработчик в очередь готовых задач.
    ///
    /// `completion` заполняется, когда задачу запустили ради результата:
    /// так [`Runtime::call_function`] получает `ret` извне планировщика.
    ///
    /// # Errors
    ///
    /// Возвращает [`RuntimeError::MissingParameter`], если вызов не передал
    /// значение объявленного параметра.
    #[tracing::instrument(level = "debug", skip(self, stream), fields(index = ?index, args = ?args, completion = ?completion))]
    pub(crate) fn enqueue_handler(
        &mut self,
        stream: Stream<'a>,
        index: usize,
        args: Vec<(String, Bound<'a>)>,
        completion: Option<Completion<'a>>,
    ) -> Result<()> {
        self.bind_handler(&stream, index, args)?;
        let mut task = Task {
            stream,
            stack: Vec::new(),
            completion,
        };
        task.block(self.program.body(index));
        self.scheduler.ready.push_back(task);
        Ok(())
    }

    /// Сколько потоков ещё не завершилось: готовых и спящих.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn pending_tasks(&self) -> usize {
        self.scheduler.ready.len()
            + self
                .scheduler
                .sleeping
                .values()
                .map(VecDeque::len)
                .sum::<usize>()
    }

    /// Исполняет поставленные задачи, пока они не кончатся или пока не
    /// истечёт бюджет виртуальных тиков [`Config::tick_limit`].
    ///
    /// Спящие задачи, чей тик лежит за бюджетом, остаются в очереди: их
    /// продолжит следующий вызов. Ошибка первого упавшего потока возвращается
    /// вызывающему, остальные потоки остаются в очередях нетронутыми.
    ///
    /// # Errors
    ///
    /// Возвращает первую неперехваченную ошибку исполняемой задачи.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn run_scheduler(&mut self) -> Result<()> {
        let horizon = self.world.tick().saturating_add(self.config.tick_limit);
        loop {
            self.wake_tasks();
            if let Some(mut task) = self.scheduler.ready.pop_front() {
                let origin = task.stream.origin.clone();
                match self.resume_task(&mut task) {
                    Ok(Some(ticks)) => {
                        let wake = self.world.tick().saturating_add(ticks);
                        self.scheduler
                            .sleeping
                            .entry(wake)
                            .or_default()
                            .push_back(task);
                    }
                    Ok(None) => {
                        if let Some(slot) = &task.completion {
                            *slot.borrow_mut() = Some(
                                task.stream
                                    .line
                                    .peek(RETURN_VARIABLE)
                                    .or_else(|| task.stream.local.peek(RETURN_VARIABLE))
                                    .flatten(),
                            );
                        }
                    }
                    Err(error) => {
                        let error = RuntimeError::called_from(origin, error);
                        // Неперехваченное исключение останавливает только свой
                        // поток: `control_call_exception` — сообщение программы
                        // о её проблеме, а не ошибка мока. Оно остаётся в
                        // журнале, а остальные потоки продолжают работу, так
                        // что видны и следующие ошибки модуля.
                        if matches!(error.root(), RuntimeError::Raised { .. }) {
                            self.log(format!("<exception: {error}>"));
                            continue;
                        }
                        return Err(error);
                    }
                }
                continue;
            }
            let Some((&next, _)) = self.scheduler.sleeping.first_key_value() else {
                return Ok(());
            };
            if next > horizon {
                return Ok(());
            }
            let current = self.world.tick();
            if next > current {
                self.world.advance(next - current);
            }
        }
    }

    /// Переносит в готовые все задачи, чей тик уже наступил.
    #[tracing::instrument(level = "debug", skip(self))]
    fn wake_tasks(&mut self) {
        let now = self.world.tick();
        while self
            .scheduler
            .sleeping
            .first_key_value()
            .is_some_and(|(&tick, _)| tick <= now)
        {
            let Some((_, mut tasks)) = self.scheduler.sleeping.pop_first() else {
                break;
            };
            self.scheduler.ready.append(&mut tasks);
        }
    }

    /// Возобновляет задачу до приостановки, завершения или ошибки.
    ///
    /// `Ok(Some(ticks))` — задача уснула на столько тиков, `Ok(None)` — она
    /// завершилась.
    #[tracing::instrument(level = "debug", skip(self, task))]
    fn resume_task(&mut self, task: &mut Task<'a>) -> Result<Option<u64>> {
        let mut result: Result<Flow> = Ok(Flow::Continue);
        while let Some(frame) = task.stack.pop() {
            match frame {
                Continuation::Block {
                    ops,
                    next,
                    mut condition,
                } => {
                    if !matches!(result, Ok(Flow::Continue)) || next >= ops.len() {
                        continue;
                    }
                    let op = &ops[next];
                    let location = format!("{}: op #{next}", task.stream.origin);
                    // Reserve the parent block before entering any child
                    // continuation: the child sits above it and is consumed
                    // first.
                    let parent = task.stack.len();
                    task.stack.push(Continuation::Block {
                        ops,
                        next: next + 1,
                        condition,
                    });
                    task.stack.push(Continuation::Location(location));
                    let outcome = self.schedule_op(task, op, &mut condition);
                    if let Some(Continuation::Block {
                        condition: slot, ..
                    }) = task.stack.get_mut(parent)
                    {
                        *slot = condition;
                    }
                    match outcome {
                        Ok(Step::Continue) => {}
                        Ok(Step::Signal(flow)) => result = Ok(flow),
                        Ok(Step::Sleep(ticks)) => return Ok(Some(ticks)),
                        Err(error) => result = Err(error),
                    }
                }
                Continuation::Location(location) => {
                    result = result.map_err(|error| RuntimeError::at(&location, error));
                }
                Continuation::Targets(targets) => task.stream.targets = targets,
                Continuation::Call {
                    line,
                    origin,
                    callee,
                } => {
                    task.stream.line = line;
                    task.stream.origin = origin;
                    result = match result {
                        Ok(Flow::Return) => Ok(Flow::Continue),
                        Err(error) => Err(RuntimeError::called_from(callee, error)),
                        other => other,
                    };
                }
                Continuation::Loop { op, mut state } => match result {
                    Ok(Flow::Continue | Flow::SkipIteration) => {
                        result = Ok(Flow::Continue);
                        match state.next(self, &mut task.stream) {
                            Ok(true) => {
                                task.stack.push(Continuation::Loop { op, state });
                                task.block(body_of(op));
                            }
                            Ok(false) => {}
                            Err(error) => result = Err(error),
                        }
                    }
                    Ok(Flow::StopRepeat) => result = Ok(Flow::Continue),
                    _ => {}
                },
                Continuation::Measure {
                    scope,
                    name,
                    start,
                    scale,
                } => {
                    if result.is_ok() {
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "виртуальные тики — небольшие целые, а `JustMC` хранит длительность числом с плавающей точкой"
                        )]
                        let elapsed = self.world.tick().saturating_sub(start) as f64;
                        self.scope_store(&task.stream, scope)
                            .set(&name, Some(value::number(elapsed * scale)));
                    }
                }
                Continuation::Catch { scope, name, kind } => {
                    if let Err(error) = &result
                        && error.matches_kind(kind)
                    {
                        self.scope_store(&task.stream, scope)
                            .set(&name, Some(value::text(error.to_string())));
                        result = Ok(Flow::Continue);
                    }
                }
                // A label is only a landmark for `control_break_label`; running
                // past it means the body finished without a break.
                Continuation::Label { .. } => {}
            }
        }
        result.map(|_| None)
    }

    /// Планирует одну операцию, не исполняя её тело синхронно.
    ///
    /// Всё, что может приостановить поток, кладётся в стек продолжений:
    /// `wait` возвращает [`Step::Sleep`], а контейнеры — `Block` с телом,
    /// выше которого лежит их собственный кадр ([`Continuation::Loop`],
    /// [`Continuation::Measure`], [`Continuation::Catch`], [`Continuation::Call`]).
    ///
    /// # Errors
    ///
    /// Возвращает ошибку проверки аргументов и ошибку действий, исполняемых
    /// сразу (условие ветвления, выборка, обычное действие).
    #[tracing::instrument(level = "debug", skip(self, task, op), fields(condition = ?condition))]
    fn schedule_op(
        &mut self,
        task: &mut Task<'a>,
        op: &'a Op<'a>,
        condition: &mut Option<bool>,
    ) -> Result<Step> {
        // Каждая запланированная операция — шаг: счётчик показывает, сколько
        // работы прогон успел сделать, и ограничивает её. Итерации повтора
        // считаются отдельно (см. `repeat::LoopState::next`): тело цикла может
        // быть пустым, и тогда планировать нечего.
        self.step()?;
        if let Some(selection) = &op.selection {
            let targets = self.selection_targets(&task.stream, &selection.selection_type)?;
            let previous = std::mem::replace(&mut task.stream.targets, targets);
            task.stack.push(Continuation::Targets(previous));
        }
        let owner = op.conditional.map_or(op.action, |c| c.action);
        schema::check_arguments(owner, op)?;

        if op.action == ActionId::Else {
            let taken = condition.take().ok_or(RuntimeError::MisplacedElse)?;
            if !taken {
                task.block(body_of(op));
            }
            return Ok(Step::Continue);
        }
        if is_branch(op.action) {
            let inverted = op.is_inverted.unwrap_or(false);
            let taken = self.condition(&mut task.stream, op.action, op)? != inverted;
            *condition = Some(taken);
            if taken {
                task.block(body_of(op));
            }
            return Ok(Step::Continue);
        }
        // Anything that is not a branch breaks the link with the previous `if`.
        *condition = None;

        match op.action {
            ActionId::ControlWait => {
                return Ok(match code::wait_ticks(self, &mut task.stream, op)? {
                    0 => Step::Continue,
                    ticks => Step::Sleep(ticks),
                });
            }
            ActionId::CallFunction => return self.enter_function(task, op),
            ActionId::RepeatForever
            | ActionId::RepeatMultiTimes
            | ActionId::RepeatOnRange
            | ActionId::RepeatOnCircle
            | ActionId::RepeatOnGrid
            | ActionId::RepeatOnPath
            | ActionId::RepeatOnSphere
            | ActionId::RepeatAdjacently
            | ActionId::RepeatForEachInList
            | ActionId::RepeatForEachMapEntry
            | ActionId::RepeatWhile => {
                let state = repeat::prepare(self, &mut task.stream, op)?;
                task.stack.push(Continuation::Loop { op, state });
                return Ok(Step::Continue);
            }
            ActionId::ControllerMeasureTime => return self.enter_measure(task, op),
            ActionId::ControllerException => return self.enter_catch(task, op),
            ActionId::ControllerLabel => {
                let args = Args::of(op);
                let label = self.text_arg(&mut task.stream, args, "label")?.into_owned();
                task.stack.push(Continuation::Label { label });
                task.block(body_of(op));
                return Ok(Step::Continue);
            }
            ActionId::ControlBreakLabel => {
                let args = Args::of(op);
                let label = self.text_arg(&mut task.stream, args, "label")?.into_owned();
                // Unwind everything up to and including the labelled frame: the
                // loops between it and here end with it.
                let Some(at) = task
                    .stack
                    .iter()
                    .rposition(|frame| matches!(frame, Continuation::Label { label: found } if *found == label))
                else {
                    return Err(RuntimeError::UnknownLabel { label });
                };
                task.stack.truncate(at);
                return Ok(Step::Continue);
            }
            ActionId::ControllerAsyncRun => {
                self.spawn_async(task, op);
                return Ok(Step::Continue);
            }
            // `controller_do_not_run` is a container whose body never runs:
            // it is how `JustMC` keeps disabled code around.
            ActionId::ControllerDoNotRun => return Ok(Step::Continue),
            _ => {}
        }

        if let Some(conditional) = op.conditional {
            return self
                .exec_conditional(&mut task.stream, op, conditional)
                .map(Step::Signal);
        }
        let outcome = crate::actions::dispatch(self, &mut task.stream, op);
        if let Err(error) = &outcome
            && self.log_uncaught_warning(task, error)
        {
            return Ok(Step::Continue);
        }
        outcome.map(Step::Signal)
    }

    /// Записывает в журнал исключение-предупреждение, которое никто не поймал.
    ///
    /// Предупреждение поток не останавливает: `control_call_exception` без
    /// `type` — это сообщение программы, и следующая операция выполняется как
    /// обычно. `false` означает «это не такое исключение»: тогда ошибку надо
    /// раскрутить до кадра [`Continuation::Catch`] или до конца потока.
    #[tracing::instrument(level = "debug", skip(self, task, error))]
    fn log_uncaught_warning(&mut self, task: &Task<'a>, error: &RuntimeError) -> bool {
        if !matches!(
            error.root(),
            RuntimeError::Raised {
                kind: ExceptionKind::Warning,
                ..
            }
        ) {
            return false;
        }
        if task.stack.iter().any(
            |frame| matches!(frame, Continuation::Catch { kind, .. } if error.matches_kind(*kind)),
        ) {
            return false;
        }
        self.log(format!("<exception: {error}>"));
        true
    }

    /// Входит в функцию: кладёт кадр вызывающего в стек и открывает тело.
    ///
    /// Тело функции продолжает **ту же** задачу, а не заводит новую: `local`
    /// общая на весь поток, и именно через неё вызываемый возвращает `ret`.
    ///
    /// Аргументы разбираются до подмены `line`, пока на месте ещё кадр
    /// вызывающего: значение аргумента — это значение переменной вызывающего.
    /// Строчное хранилище вызывающего при этом остаётся под рукой — на него
    /// ссылаются `variable`-параметры (см. [`Self::bind_handler`]).
    #[tracing::instrument(level = "debug", skip(self, task, op))]
    fn enter_function(&mut self, task: &mut Task<'a>, op: &'a Op<'a>) -> Result<Step> {
        let args = Args::of(op);
        let name = self
            .text_arg(&mut task.stream, args, "function_name")?
            .into_owned();
        let index = self.program.function_index(&name)?;
        let declared = self.program.parameters(index).to_vec();
        let parameters = code::parameters(args)?;
        let caller_line = task.stream.line.clone();
        let parameters =
            self.resolve_arguments(&mut task.stream, Some(&caller_line), &declared, parameters)?;
        let line = std::mem::take(&mut task.stream.line);
        let callee = format!("function {name}");
        let origin = std::mem::replace(&mut task.stream.origin, callee.clone());
        task.stack.push(Continuation::Call {
            line,
            origin,
            callee,
        });
        self.bind_handler(&task.stream, index, parameters)?;
        task.block(self.program.body(index));
        Ok(Step::Continue)
    }

    /// Входит в `controller_measure_time`: засекает тик и открывает тело.
    #[tracing::instrument(level = "debug", skip(self, task, op))]
    fn enter_measure(&mut self, task: &mut Task<'a>, op: &'a Op<'a>) -> Result<Step> {
        let args = Args::of(op);
        let (scope, name) = self.target_of(&task.stream, args, "variable")?;
        // Один тик `JustMC` — 50 мс, отсюда и множители: длительность
        // записывается в выбранных единицах измерения.
        let scale = match self.optional_enum_arg(&mut task.stream, args, "duration")? {
            Some("MICROSECONDS") => 50_000.0,
            Some("NANOSECONDS") => 50_000_000.0,
            _ => 50.0,
        };
        task.stack.push(Continuation::Measure {
            scope,
            name,
            start: self.world.tick(),
            scale,
        });
        task.block(body_of(op));
        Ok(Step::Continue)
    }

    /// Входит в `controller_exception`: ставит ловушку и открывает тело.
    #[tracing::instrument(level = "debug", skip(self, task, op))]
    fn enter_catch(&mut self, task: &mut Task<'a>, op: &'a Op<'a>) -> Result<Step> {
        let args = Args::of(op);
        let (scope, name) = self.target_of(&task.stream, args, "variable")?;
        let kind = self
            .optional_enum_arg(&mut task.stream, args, "exception_type")?
            .and_then(ExceptionKind::parse)
            .unwrap_or(ExceptionKind::All);
        task.stack.push(Continuation::Catch { scope, name, kind });
        task.block(body_of(op));
        Ok(Step::Continue)
    }

    /// Запускает тело `controller_async_run` отдельной задачей.
    ///
    /// Локальные переменные копируются: тело живёт своим потоком и не должно
    /// менять `local` родителя. Цели, выделение и данные события наследуются,
    /// а родитель продолжается сразу же, не дожидаясь тела.
    #[tracing::instrument(level = "debug", skip(self, task, op))]
    fn spawn_async(&mut self, task: &Task<'a>, op: &'a Op<'a>) {
        let mut frame = spawn_stream(task.stream.targets.clone(), task.stream.origin.clone());
        frame.selection.clone_from(&task.stream.selection);
        frame.local = Shared::with_vars(task.stream.local.snapshot());
        frame.chat_message.clone_from(&task.stream.chat_message);
        frame.event_item.clone_from(&task.stream.event_item);
        frame.event_slot = task.stream.event_slot;
        frame
            .inventory_title
            .clone_from(&task.stream.inventory_title);
        let mut child = Task {
            stream: frame,
            stack: Vec::new(),
            completion: None,
        };
        child.block(body_of(op));
        self.scheduler.ready.push_back(child);
    }
}
