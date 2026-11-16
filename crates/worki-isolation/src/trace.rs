//! Reports the commands worki runs to whoever wants to show them (an activity's log), on the
//! thread that runs them. Async code carries the sink across threads itself (`worki_core::trace`).

use std::cell::RefCell;
use std::ffi::OsStr;
use std::process::Command;
use std::sync::Arc;

pub type Sink = Arc<dyn Fn(String) + Send + Sync>;

const MAX_LEN: usize = 400;

thread_local! {
    static CURRENT: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

pub fn current() -> Option<Sink> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Runs `f` with commands on this thread reported to `sink`.
pub fn with_sink<R>(sink: Option<Sink>, f: impl FnOnce() -> R) -> R {
    let before = CURRENT.with(|c| c.replace(sink));
    let result = f();
    CURRENT.with(|c| *c.borrow_mut() = before);
    result
}

/// Reports a command about to run to `sink`.
pub fn report_to(sink: &Sink, program: &OsStr, args: impl IntoIterator<Item = impl AsRef<OsStr>>) {
    sink(command_line(program, args));
}

pub trait Traced {
    /// Reports the command to this thread's sink, if any, before it runs.
    fn traced(&mut self) -> &mut Self;
}

impl Traced for Command {
    fn traced(&mut self) -> &mut Self {
        if let Some(sink) = current() {
            report_to(&sink, self.get_program(), self.get_args());
        }
        self
    }
}

/// The command as a shell line, values of secret-looking `NAME=value` arguments hidden and
/// long scripts shortened.
fn command_line(program: &OsStr, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> String {
    let program = program.to_string_lossy();
    let name = program.rsplit('/').next().unwrap_or(&program).to_owned();
    let mut line = std::iter::once(name)
        .chain(
            args.into_iter()
                .map(|a| quote(&redact(&a.as_ref().to_string_lossy()))),
        )
        .collect::<Vec<_>>()
        .join(" ");
    if line.chars().count() > MAX_LEN {
        line = line.chars().take(MAX_LEN).collect::<String>() + "…";
    }
    line
}

fn redact(arg: &str) -> String {
    let Some((name, _)) = arg.split_once('=') else {
        return arg.to_owned();
    };
    let upper = name.to_ascii_uppercase();
    let secret = ["TOKEN", "KEY", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL"]
        .iter()
        .any(|word| upper.contains(word));
    let plain_name =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if secret && plain_name {
        format!("{name}=…")
    } else {
        arg.to_owned()
    }
}

fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%…".contains(c));
    if plain {
        arg.to_owned()
    } else {
        crate::ssh::shell_quote(&arg.replace('\n', " "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn quotes_and_hides_secrets() {
        let line = command_line(
            OsStr::new("/usr/local/bin/docker"),
            [
                "run",
                "-e",
                "API_TOKEN=abc",
                "-e",
                "CLAUDE_CONFIG_DIR=/x y",
                "bash",
                "-lc",
                "echo 'hi'",
            ],
        );
        assert_eq!(
            line,
            r"docker run -e API_TOKEN=… -e 'CLAUDE_CONFIG_DIR=/x y' bash -lc 'echo '\''hi'\'''"
        );
    }

    #[test]
    fn reports_to_the_thread_sink_only_inside_it() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink: Sink = {
            let seen = seen.clone();
            Arc::new(move |line| seen.lock().unwrap().push(line))
        };
        Command::new("/bin/echo").arg("outside").traced();
        with_sink(Some(sink), || {
            Command::new("/bin/echo").arg("inside").traced();
        });
        Command::new("/bin/echo").arg("after").traced();
        assert_eq!(*seen.lock().unwrap(), ["echo inside"]);
    }
}
