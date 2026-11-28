use std::future::Future;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use worki_isolation::Launch;

use crate::model::LogLine;
use crate::{Error, Result};

/// Runs a non-interactive launch (setup script, installer), streaming each output line to `log`.
pub async fn run_logged(
    launch: Launch,
    log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
) -> Result<()> {
    match run(launch, Stdio::null(), log, std::future::pending()).await? {
        Some(0) => Ok(()),
        code => Err(Error::CommandFailed(code)),
    }
}

/// Runs a non-interactive launch and returns its output. On failure, the error carries the end
/// of what it printed.
pub async fn run_captured(launch: Launch) -> Result<String> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let err = Arc::new(Mutex::new(Vec::new()));
    let (o, e) = (out.clone(), err.clone());
    let result = run_logged(launch, move |line| match line {
        LogLine::Stdout(l) => o.lock().unwrap().push(l),
        LogLine::Stderr(l) => e.lock().unwrap().push(l),
        LogLine::Info(_) | LogLine::Command(_) => {}
    })
    .await;
    let out = out.lock().unwrap().join("\n");
    match result {
        Ok(()) => Ok(out),
        Err(Error::CommandFailed(code)) => {
            let err = err.lock().unwrap();
            let lines = if err.is_empty() {
                out.lines().map(str::to_owned).collect()
            } else {
                err.clone()
            };
            let tail = lines[lines.len().saturating_sub(5)..].join("\n");
            Err(Error::Isolation(if tail.trim().is_empty() {
                format!("the agent stopped with exit code {}", code.unwrap_or(-1))
            } else {
                tail
            }))
        }
        Err(e) => Err(e),
    }
}

async fn pump<R: AsyncRead + Unpin>(
    reader: Option<R>,
    log: impl Fn(LogLine),
    wrap: fn(String) -> LogLine,
) {
    let Some(reader) = reader else { return };
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    while reader
        .read_until(b'\n', &mut buf)
        .await
        .is_ok_and(|n| n > 0)
    {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\n', '\r']);
        log(wrap(
            line.rsplit('\r').next().unwrap_or_default().to_owned(),
        ));
        buf.clear();
    }
}

pub async fn run_streamed(
    launch: Launch,
    log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    stop: impl Future<Output = ()>,
) -> Result<Option<i32>> {
    run(launch, Stdio::piped(), log, stop).await
}

async fn run(
    launch: Launch,
    stdin: Stdio,
    log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    stop: impl Future<Output = ()>,
) -> Result<Option<i32>> {
    let mut command = Command::from(launch.command());
    crate::trace::report(&command);
    let mut child = command
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdin = child.stdin.take();
    let out = tokio::spawn(pump(child.stdout.take(), log.clone(), LogLine::Stdout));
    let err = tokio::spawn(pump(child.stderr.take(), log, LogLine::Stderr));

    let code = tokio::select! {
        status = child.wait() => status?.code(),
        () = stop => {
            drop(stdin);
            if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                child.kill().await?;
            }
            None
        }
    };
    let (out_abort, err_abort) = (out.abort_handle(), err.abort_handle());
    if tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(out, err) })
        .await
        .is_err()
    {
        out_abort.abort();
        err_abort.abort();
    }
    Ok(code)
}

/// Runs `$WORKI_SNIPPET` as its own process group and kills that group once stdin closes, so
/// stopping reaches what the snippet started inside a container or over SSH, not just the local
/// client. Job control needs bash; its own job notices go to /dev/null, the snippet's stderr to
/// fd 3.
pub const SNIPPET_WRAPPER: &str = "exec bash -c 'exec 3>&2 2>/dev/null; set -m; \
    ( eval \"$WORKI_SNIPPET\" ) </dev/null 2>&3 3>&- & p=$!; \
    ( cat >/dev/null; kill -TERM -- -$p ) 3>&- & w=$!; \
    wait $p; s=$?; kill -- -$w; exit $s'";

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use worki_isolation::{Isolation, LaunchRequest};

    use super::*;

    fn snippet(command: &str) -> Launch {
        let req = LaunchRequest {
            cwd: std::env::temp_dir(),
            command: Some(SNIPPET_WRAPPER.into()),
            interactive: false,
            stdin: true,
            env: vec![("WORKI_SNIPPET".into(), command.into())],
            session: None,
        };
        Isolation::None.backend().launch(&req).unwrap()
    }

    fn collect() -> (
        Arc<Mutex<Vec<String>>>,
        impl Fn(LogLine) + Send + Sync + Clone,
    ) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        (lines, move |line| match line {
            LogLine::Stdout(l) => sink.lock().unwrap().push(l),
            LogLine::Stderr(l) => sink.lock().unwrap().push(format!("err: {l}")),
            _ => {}
        })
    }

    #[tokio::test]
    async fn snippet_output_and_exit_code() {
        let (lines, log) = collect();
        let code = run_streamed(
            snippet("echo out; echo oops >&2; exit 3"),
            log,
            std::future::pending(),
        )
        .await
        .unwrap();
        assert_eq!(code, Some(3));
        let mut lines = lines.lock().unwrap().clone();
        lines.sort();
        assert_eq!(lines, ["err: oops", "out"]);
    }

    #[tokio::test]
    async fn stopping_ends_what_the_snippet_started() {
        let marker = format!("worki-snippet-test-{}", std::process::id());
        let (_, log) = collect();
        let started = Instant::now();
        let code = run_streamed(
            snippet(&format!("sh -c 'sleep 300; : {marker}' & sleep 300")),
            log,
            tokio::time::sleep(Duration::from_millis(500)),
        )
        .await
        .unwrap();
        assert_eq!(code, None);
        assert!(started.elapsed() < Duration::from_secs(4));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let left = std::process::Command::new("pgrep")
            .args(["-f", &marker])
            .output()
            .unwrap();
        assert!(
            left.stdout.is_empty(),
            "still running: {}",
            String::from_utf8_lossy(&left.stdout)
        );
    }

    #[tokio::test]
    async fn binary_output_does_not_stall() {
        let (lines, log) = collect();
        let code = run_streamed(
            snippet("head -c 200000 /dev/urandom; echo; echo done"),
            log,
            std::future::pending(),
        )
        .await
        .unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(
            lines.lock().unwrap().last().map(String::as_str),
            Some("done")
        );
    }
}
