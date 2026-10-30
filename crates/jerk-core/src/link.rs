use crate::cli::{Ctx, Io};
use crate::config::Entry;
use crate::exit::Exit;
use crate::run::{FILE_REFUSED, REFUSED};
use serde_json::{json, Value};
use std::io::Write;

pub struct Progress<'a> {
    out: &'a mut dyn Write,
    line: Vec<u8>,
    pub last: Option<String>,
    pub stream: bool,
}

impl<'a> Progress<'a> {
    pub fn new(out: &'a mut dyn Write, stream: bool) -> Progress<'a> {
        Progress {
            out,
            line: Vec::new(),
            last: None,
            stream,
        }
    }

    fn emit(&mut self) {
        let text = String::from_utf8_lossy(&self.line).to_string();
        self.line.clear();
        let message = text.trim().trim_start_matches("jerk: ").trim();
        if message.is_empty() || message.starts_with("Run jerk --help") {
            return;
        }
        if self.stream {
            let _ = writeln!(
                self.out,
                "{}",
                json!({ "event": "progress", "message": message })
            );
            let _ = self.out.flush();
        }
        self.last = Some(message.to_string());
    }

    pub fn finish(&mut self) {
        if !self.line.is_empty() {
            self.emit();
        }
    }
}

impl Write for Progress<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for byte in buf {
            if *byte == b'\n' {
                self.emit();
            } else {
                self.line.push(*byte);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn bare(provider: &str, message: &str, io: &mut Io<'_>) -> Exit {
    let message = message.trim_start_matches("jerk: ");
    let _ = writeln!(
        io.out,
        "{}",
        json!({ "provider": provider, "message": message })
    );
    Exit::Usage
}

pub fn strip(args: &[String]) -> Option<(Vec<String>, Option<String>)> {
    let mut found = false;
    let mut rest = Vec::new();
    let mut origin = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--via-link" {
            found = true;
        } else if arg == "--seven-origin" {
            origin = iter.next().cloned();
        } else if let Some(value) = arg.strip_prefix("--seven-origin=") {
            origin = Some(value.to_string());
        } else {
            rest.push(arg.clone());
        }
    }
    found.then_some((rest, origin))
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(String::from)
}

fn status(entry: &Entry, message: String, extra: &[(&str, Value)]) -> Value {
    let mut doc = json!({ "provider": entry.id, "env": entry.provider.env, "message": message });
    for (k, v) in extra {
        doc[*k] = v.clone();
    }
    doc
}

fn refuse(
    entry: &Entry,
    verb: &str,
    args: &[String],
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Option<Exit> {
    let step = match (&entry.provider.create, verb) {
        (Some(chain), "create") => {
            let mut quiet = Io {
                out: &mut std::io::sink(),
                err: &mut std::io::sink(),
            };
            let picked = crate::dispatch::pick(
                entry,
                "create",
                chain,
                &entry.provider.params,
                ctx,
                &mut quiet,
            );
            if matches!(picked, Err(Exit::Refused)) {
                let message = format!("{}: {REFUSED}", entry.id);
                let _ = writeln!(io.out, "{}", status(entry, message, &[]));
                return Some(Exit::Refused);
            }
            Some(picked.ok())
        }
        _ => None,
    };
    let minting = matches!(&step, Some(Some(v)) if v.parse.is_some());
    let terminal_only = match &step {
        Some(Some(v)) => !v.oauth && !v.link,
        Some(None) => false,
        None => entry.oauth_config.is_none(),
    };
    let (code, message) = match verb {
        "create" if args.iter().any(|a| a == "--force") => (
            Exit::Usage,
            "create --force is not allowed over the link".to_string(),
        ),
        "create" if minting => (
            Exit::Usage,
            format!(
                "{} create mints a token without a sign-in page, which the link refuses",
                entry.id
            ),
        ),
        "create" if terminal_only => (
            Exit::Unsupported,
            format!(
                "{} create needs a terminal; run jerk {} create on the host",
                entry.id, entry.id
            ),
        ),
        "create" | "lease" => return None,
        "drop" if args.iter().any(|a| a == "--lease") => return None,
        "drop" => (
            Exit::Usage,
            "drop over the link only takes --lease; a CLI sign-out never runs from the link"
                .to_string(),
        ),
        _ => (
            Exit::Usage,
            "--via-link only applies to create, lease and drop --lease".to_string(),
        ),
    };
    let _ = writeln!(io.out, "{}", status(entry, message, &[]));
    Some(code)
}

pub fn run(entry: &Entry, verb: &str, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    if let Some(code) = refuse(entry, verb, args, ctx, io) {
        return code;
    }
    ctx.stdin_terminal = false;
    ctx.stdout_terminal = false;
    ctx.via_link = true;
    let mut out = Vec::new();
    let mut progress = Progress::new(io.out, verb == "create");
    let mut code = {
        let mut inner = Io {
            out: &mut out,
            err: &mut progress,
        };
        match verb {
            "create" => crate::dispatch::create(entry, args, ctx, &mut inner),
            "lease" => crate::fnox::lease(entry, args, ctx, &mut inner),
            _ => crate::dispatch::drop(entry, args, ctx, &mut inner),
        }
    };
    progress.finish();
    let last = progress.last.take();
    drop(progress);
    let text = String::from_utf8_lossy(&out).to_string();
    let fallback = if code == Exit::Ok { "ok" } else { "failed" };
    let doc = match verb {
        "lease" if code == Exit::Ok => {
            let lease: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
            let _ = writeln!(io.out, "{lease}");
            status(
                entry,
                "leased".to_string(),
                &[("expires_at", lease["expires_at"].clone())],
            )
        }
        "lease" => status(entry, last.unwrap_or_else(|| fallback.to_string()), &[]),
        _ => {
            let extra = if verb == "create" && code == Exit::Ok {
                crate::dispatch::details(entry, args, ctx).unwrap_or_else(|refused| {
                    code = refused;
                    Vec::new()
                })
            } else {
                Vec::new()
            };
            let message = match code {
                Exit::Refused => last
                    .filter(|line| line.ends_with(FILE_REFUSED))
                    .unwrap_or_else(|| format!("{}: {REFUSED}", entry.id)),
                _ => last_line(&text)
                    .or(last)
                    .unwrap_or_else(|| fallback.to_string()),
            };
            status(entry, message, &extra)
        }
    };
    let _ = writeln!(io.out, "{doc}");
    code
}
