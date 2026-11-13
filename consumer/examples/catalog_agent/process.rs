//! Bounded synchronous child driving, not a VM scheduler or a process sandbox.
use std::{
    ffi::OsString,
    io::Read,
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const STDERR_LIMIT: usize = 128 * 1024;

pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    status: Option<ExitStatus>,
    problem: Option<&'static str>,
}

impl Captured {
    pub fn require_success(&self) -> Result<(), String> {
        if let Some(problem) = self.problem {
            return Err(problem.into());
        }
        if self.status.is_some_and(|status| status.success()) {
            Ok(())
        } else {
            Err(
                "host exited unsuccessfully; see captured stderr; side effects are not retried"
                    .into(),
            )
        }
    }
}

enum ReadEvent {
    Chunk { stdout: bool, bytes: Vec<u8> },
    End { failed: bool },
}

fn reader(
    mut pipe: impl Read + Send + 'static,
    stdout: bool,
    limit: usize,
    tx: mpsc::Sender<ReadEvent>,
) {
    thread::spawn(move || {
        let mut remaining = limit + 1;
        let mut buffer = [0; 8192];
        while remaining != 0 {
            let capacity = remaining.min(buffer.len());
            match pipe.read(&mut buffer[..capacity]) {
                Ok(0) => break,
                Ok(count) => {
                    remaining -= count;
                    // Send bounded chunks promptly so timeouts retain already-read evidence.
                    if tx
                        .send(ReadEvent::Chunk {
                            stdout,
                            bytes: buffer[..count].to_vec(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    let _ = tx.send(ReadEvent::End { failed: true });
                    return;
                }
            }
        }
        let _ = tx.send(ReadEvent::End { failed: false });
    });
}

pub fn capture(
    host: &Path,
    args: &[OsString],
    stdout_limit: usize,
    timeout: Duration,
) -> Result<Captured, String> {
    let mut child = Command::new(host)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start host: {error}"))?;
    let (tx, rx) = mpsc::channel();
    reader(
        child.stdout.take().expect("piped stdout"),
        true,
        stdout_limit,
        tx.clone(),
    );
    reader(
        child.stderr.take().expect("piped stderr"),
        false,
        STDERR_LIMIT,
        tx,
    );
    let mut captured = Captured {
        stdout: Vec::new(),
        stderr: Vec::new(),
        status: None,
        problem: None,
    };
    let mut streams = 0;
    let started = Instant::now();
    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                ReadEvent::Chunk { stdout, bytes } => {
                    let (output, limit) = if stdout {
                        (&mut captured.stdout, stdout_limit)
                    } else {
                        (&mut captured.stderr, STDERR_LIMIT)
                    };
                    output.extend(bytes);
                    if output.len() > limit {
                        captured.problem =
                            Some("host output exceeds its byte limit; outcome may be partial");
                    }
                }
                ReadEvent::End { failed } => {
                    streams += 1;
                    if failed {
                        captured.problem = Some("cannot read host output; outcome may be partial");
                    }
                }
            }
        }
        if captured.status.is_none() {
            match child.try_wait() {
                Ok(status) => captured.status = status,
                Err(_) => {
                    captured.problem =
                        Some("cannot inspect host exit status; outcome may be partial")
                }
            }
        }
        if streams == 2 && captured.status.is_some() {
            break;
        }
        if started.elapsed() >= timeout {
            captured.problem = Some("host deadline exceeded; outcome may be partial");
        }
        if captured.problem.is_some() {
            // Stop only the directly launched process. Killing cannot roll back native work.
            if captured.status.is_none() {
                let _ = child.kill();
                captured.status = child.wait().ok();
            }
            // Do not join readers: a misbehaving host could leave descendant-held pipes open.
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(captured)
}
