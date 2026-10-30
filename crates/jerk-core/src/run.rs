use crate::paths::Os;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LAUNCHER_REFUSED: &[char] = &['%', '&', '|', '<', '>', '^', '"', '!', '(', ')'];
const PATH_REFUSED: &[char] = &['%', '"', '!', '^'];
pub const REFUSED: &str =
    "a .cmd or .bat launcher cannot take a character in its path or arguments";

pub struct Opts<'a> {
    pub interactive: bool,
    pub capture: bool,
    pub echo: bool,
    pub timeout: Option<Duration>,
    pub remove_env: &'a [String],
    pub input: Option<&'a str>,
    pub on_stderr: Option<&'a dyn Fn(&str)>,
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
    Refused,
}

pub trait Runner {
    fn run(&self, program: &Path, args: &[String], opts: &Opts<'_>) -> Outcome;
}

pub struct Process {
    pub os: Os,
}

fn unsafe_text(text: &str) -> bool {
    text.contains(|c: char| c.is_control() || LAUNCHER_REFUSED.contains(&c))
}

fn unsafe_path(path: &str) -> bool {
    path.contains(|c: char| c.is_control() || PATH_REFUSED.contains(&c))
}

fn file_name(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

fn is_launcher(program: &str) -> bool {
    file_name(program).split(':').any(|part| {
        let part = part.trim_end_matches(['.', ' ']);
        part.ends_with(".cmd") || part.ends_with(".bat")
    })
}

pub fn launcher_refuses(os: Os, program: &Path, args: &[String]) -> bool {
    let program = program.to_string_lossy().to_ascii_lowercase();
    os == Os::Windows
        && is_launcher(&program)
        && (unsafe_path(&program)
            || program.contains("::")
            || file_name(&program).contains(':')
            || args.iter().any(|arg| unsafe_text(arg)))
}

fn drain_lines(
    pipe: impl Read + Send + 'static,
    lines: std::sync::mpsc::Sender<String>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let mut reader = std::io::BufReader::new(pipe);
        let mut line = String::new();
        while matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
            let _ = lines.send(line.trim_end().to_string());
            text.push_str(&line);
            line.clear();
        }
        text
    })
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

fn drain_echo(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut screen = std::io::stderr();
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let _ = screen.write_all(&chunk[..n]);
            let _ = screen.flush();
            bytes.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

impl Runner for Process {
    fn run(&self, program: &Path, args: &[String], opts: &Opts<'_>) -> Outcome {
        if launcher_refuses(self.os, program, args) {
            return Outcome::Refused;
        }
        let mut command = Command::new(program);
        command.args(args);
        for name in opts.remove_env {
            command.env_remove(name);
        }
        if opts.interactive {
            let stdout = if opts.capture {
                Stdio::piped()
            } else {
                Stdio::inherit()
            };
            command
                .stdin(Stdio::inherit())
                .stdout(stdout)
                .stderr(Stdio::inherit());
        } else {
            let stdin = if opts.input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            };
            command
                .stdin(stdin)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return Outcome::Failed(e.to_string()),
        };
        if let (Some(text), Some(mut stdin)) = (opts.input, child.stdin.take()) {
            let _ = stdin.write_all(text.as_bytes());
        }
        let out = child.stdout.take().map(|pipe| match opts.echo {
            true => drain_echo(pipe),
            false => drain(pipe),
        });
        let (lines, received) = std::sync::mpsc::channel();
        let err = child.stderr.take().map(|pipe| drain_lines(pipe, lines));
        let forward = || {
            while let Ok(line) = received.try_recv() {
                if let Some(on_stderr) = opts.on_stderr {
                    on_stderr(&line);
                }
            }
        };
        let deadline = opts.timeout.map(|t| Instant::now() + t);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if deadline.is_some_and(|d| Instant::now() >= d) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::TimedOut;
                }
                _ => {
                    forward();
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let take = |h: Option<std::thread::JoinHandle<String>>| {
            h.map(|h| h.join().unwrap_or_default()).unwrap_or_default()
        };
        let stdout = take(out);
        let stderr = take(err);
        forward();
        Outcome::Exited {
            code: status.code().unwrap_or(1),
            stdout,
            stderr,
        }
    }
}
