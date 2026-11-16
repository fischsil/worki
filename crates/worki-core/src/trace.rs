//! The command sink of the work in progress (an activity), carried through async code and into
//! the blocking threads it hands work to, where `worki_isolation::trace` reports commands.

use std::future::Future;

pub use worki_isolation::trace::Sink;
use worki_isolation::trace::{report_to, with_sink};

tokio::task_local! {
    static SINK: Sink;
}

fn current() -> Option<Sink> {
    SINK.try_with(Sink::clone)
        .ok()
        .or_else(worki_isolation::trace::current)
}

/// Runs `work` with the commands it runs reported to `sink`.
pub async fn scope<F: Future>(sink: Sink, work: F) -> F::Output {
    SINK.scope(sink, work).await
}

/// `f`, to run on another thread with this task's sink.
pub fn carry<R>(f: impl FnOnce() -> R) -> impl FnOnce() -> R {
    let sink = current();
    move || with_sink(sink, f)
}

pub(crate) fn report(cmd: &tokio::process::Command) {
    if let Some(sink) = current() {
        let cmd = cmd.as_std();
        report_to(&sink, cmd.get_program(), cmd.get_args());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use worki_isolation::trace::Traced;

    use super::*;

    #[tokio::test]
    async fn follows_work_into_blocking_threads() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink: Sink = {
            let seen = seen.clone();
            Arc::new(move |line| seen.lock().unwrap().push(line))
        };
        scope(sink, async {
            report(&tokio::process::Command::new("/usr/bin/git"));
            tokio::task::spawn_blocking(carry(|| {
                std::process::Command::new("/bin/echo").arg("hi").traced();
            }))
            .await
            .unwrap();
        })
        .await;
        tokio::task::spawn_blocking(carry(|| {
            std::process::Command::new("/bin/echo")
                .arg("outside")
                .traced();
        }))
        .await
        .unwrap();
        assert_eq!(*seen.lock().unwrap(), ["git", "echo hi"]);
    }
}
