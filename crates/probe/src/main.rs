//! `mindrune-probe`: checks the stream-json protocol against a real `claude`
//! and turns recordings into test fixtures.

mod events;
mod repl;
mod scrub;

use std::path::PathBuf;
use std::process::ExitCode;

use mr_claude::ClaudeAgent;

const USAGE: &str = "\
usage:
  mindrune-probe raw --cwd DIR [--program PATH] [--resume ID] [--fork] [--model M]
                        [--mode MODE] [--record FILE] [--full-login] [--arg VALUE]...
  mindrune-probe events --cwd DIR [--model M]
  mindrune-probe scrub IN OUT";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("raw") => match parse_raw(&args[1..]) {
            Ok(options) => finish(repl::run_raw(options)),
            Err(message) => usage_error(&message),
        },
        Some("events") => match parse_events(&args[1..]) {
            Ok((cwd, model)) => finish(events::run(&ClaudeAgent::from_environment(), cwd, model)),
            Err(message) => usage_error(&message),
        },
        Some("scrub") if args.len() == 3 => finish(scrub_command(&args[1], &args[2])),
        Some("scrub") => usage_error("scrub takes IN and OUT"),
        _ => usage_error(""),
    }
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("mindrune-probe: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    if !message.is_empty() {
        eprintln!("mindrune-probe: {message}");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn parse_raw(args: &[String]) -> Result<repl::RawOptions, String> {
    let mut cwd = None;
    let mut program = PathBuf::from("claude");
    let mut options = repl::RawOptions {
        cwd: PathBuf::new(),
        program: PathBuf::new(),
        resume: None,
        fork: false,
        model: None,
        mode: None,
        record: None,
        full_login: false,
        extra_args: Vec::new(),
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--cwd" => cwd = Some(PathBuf::from(value("--cwd")?)),
            "--program" => program = PathBuf::from(value("--program")?),
            "--resume" => options.resume = Some(value("--resume")?),
            "--model" => options.model = Some(value("--model")?),
            "--mode" => options.mode = Some(value("--mode")?),
            "--record" => options.record = Some(PathBuf::from(value("--record")?)),
            // Taken as it is, also when it starts with `--`.
            "--arg" => options.extra_args.push(value("--arg")?),
            "--fork" => options.fork = true,
            "--full-login" => options.full_login = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    options.cwd = cwd.ok_or("--cwd is required")?;
    options.program = absolute_program(program)?;
    Ok(options)
}

fn parse_events(args: &[String]) -> Result<(PathBuf, Option<String>), String> {
    let mut cwd = None;
    let mut model = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--cwd" => cwd = Some(PathBuf::from(value("--cwd")?)),
            "--model" => model = Some(value("--model")?),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok((cwd.ok_or("--cwd is required")?, model))
}

/// The child starts in `--cwd`, so a relative path to the program is resolved
/// against the probe's own directory first. A bare command name has no
/// directory part and is left for the PATH lookup.
fn absolute_program(program: PathBuf) -> Result<PathBuf, String> {
    let has_directory = program
        .parent()
        .is_some_and(|dir| !dir.as_os_str().is_empty());
    if !has_directory || program.is_absolute() {
        return Ok(program);
    }
    std::path::absolute(&program).map_err(|e| format!("cannot resolve {}: {e}", program.display()))
}

fn scrub_command(input: &str, output: &str) -> Result<(), String> {
    let recording = std::fs::read(input)
        .map_err(|e| format!("cannot read {input}: {e}"))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())?;
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        eprintln!("mindrune-probe: HOME is not set; the home directory is not replaced");
    }
    let fixture = scrub::scrub_lines(&recording, &home);
    std::fs::write(output, fixture).map_err(|e| format!("cannot write {output}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_program_path_is_made_absolute_and_a_bare_name_is_left_alone() {
        assert_eq!(absolute_program("claude".into()), Ok("claude".into()));
        assert_eq!(
            absolute_program("/opt/claude".into()),
            Ok("/opt/claude".into())
        );
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_program("a/stub.sh".into()),
            Ok(cwd.join("a/stub.sh"))
        );
    }

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn every_arg_is_kept_in_its_order_even_one_that_starts_with_dashes() {
        let options = parse_raw(&words(
            "--arg --replay-user-messages --cwd /p --arg --effort --arg low",
        ))
        .unwrap();
        assert_eq!(
            options.extra_args,
            words("--replay-user-messages --effort low")
        );
        assert_eq!(options.cwd, PathBuf::from("/p"));
        assert_eq!(
            parse_raw(&words("--cwd /p")).unwrap().extra_args,
            Vec::<String>::new()
        );
        assert_eq!(
            parse_raw(&words("--cwd /p --arg")).err(),
            Some("--arg needs a value".to_string())
        );
    }
}
