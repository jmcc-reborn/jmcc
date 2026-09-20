//! Reproducible fuzz harness for `jmcmock` over a compiled module.
//!
//! Build and run:
//! `cargo run -p jmcmock --example fuzz_run -- <module.json> [seed] [iters]`
//!
//! It fires pseudo-random events with pseudo-random event data and reports what
//! the mock says. Panics and mock-internal inconsistencies are what matter; a
//! `Raised`/validation error coming from the program under test is expected.

#![expect(
    clippy::print_stderr,
    reason = "the harness reports progress and failures on stderr, leaving stdout free"
)]

use jmcdata::module::LineValue;
use jmcmock::{Config, EventData, Program, Runtime, Unimplemented};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: fuzz_run <module.json> [seed] [iters]");
    let seed: u64 = args.next().map_or(1, |s| s.parse().expect("seed"));
    let iters: u32 = args.next().map_or(200, |s| s.parse().expect("iters"));
    let text = std::fs::read_to_string(&path).expect("read module");
    let program = Program::parse(&text).expect("parse module");

    let mut names: Vec<String> = Vec::new();
    for handler in &program.module().handlers {
        if let LineValue::Event { event } = &handler.line_value {
            let name = jmcmock::event_name(*event);
            if !names.iter().any(|n| n == &name) {
                names.push(name);
            }
        }
    }
    names.sort();
    eprintln!("fuzz: {} distinct events", names.len());

    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    let mut errors = 0usize;
    for i in 0..iters {
        let event = names[(next() as usize) % names.len()].clone();
        let chat = format!("fuzz{}", next() % 1000);
        let slot = ((next() % 9) as f64) + 1.0;
        let config = Config {
            unimplemented: Unimplemented::Ignore,
            step_limit: 3_000_000,
            log_limit: None,
            ..Config::default()
        };
        let mut runtime = match Runtime::with_config(&program, config) {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[{i}] runtime init failed for {event}: {e}");
                errors += 1;
                continue;
            }
        };
        let data = EventData {
            chat_message: Some(chat),
            slot: Some(slot),
            ..EventData::default()
        };
        if let Err(e) = runtime.fire_event_with(&event, data) {
            errors += 1;
            eprintln!("[{i}] {event}: {e}");
        }
    }
    eprintln!("fuzz: done, {errors} reported failures");
}
