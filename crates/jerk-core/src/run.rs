use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Opts<'a> {
    pub interactive: bool,
    pub timeout: Option<Duration>,
    pub remove_env: &'a [String],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Exited {
        code: i32,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    Failed(String),
}

pub trait Runner {
    fn run(&self, program: &Path, args: &[String], opts: &Opts<'_>) -> Outcome;
}

pub struct Process;

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

impl Runner for Process {
    fn run(&self, program: &Path, args: &[String], opts: &Opts<'_>) -> Outcome {
        let mut command = Command::new(program);
        command.args(args);
        for name in opts.remove_env {
            command.env_remove(name);
        }
        if opts.interactive {
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        } else {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return Outcome::Failed(e.to_string()),
        };
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        let deadline = opts.timeout.map(|t| Instant::now() + t);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if deadline.is_some_and(|d| Instant::now() >= d) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::TimedOut;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        let take = |h: Option<std::thread::JoinHandle<String>>| {
            h.map(|h| h.join().unwrap_or_default()).unwrap_or_default()
        };
        Outcome::Exited {
            code: status.code().unwrap_or(1),
            stdout: take(out),
            stderr: take(err),
        }
    }
}
