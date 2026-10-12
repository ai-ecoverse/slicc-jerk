use crate::exit::Exit;
use std::io::Write;

pub(crate) const HELP: &str =
    "jerk: get, list, create and drop OAuth tokens through each provider's own CLI

Usage:
  jerk list [--json]
  jerk <provider> get    [--scope S] [--reveal] [--refresh] [--login] [--json]
  jerk <provider> list   [--json]
  jerk <provider> create [--scope S] [--force]
  jerk <provider> drop   [<id>] [--lease] [--yes]
  jerk <provider> lease
  jerk <provider> sync --fnox <SECRET> --provider <fnox provider>
  jerk config [path|check|fnox|show <provider>]
  jerk intercept --authorize-url <url> --redirect <url>

Options:
  -h, --help     show this help
  -V, --version  show the version

Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in needed, 4 not available here, 5 unsupported
";

pub(crate) const VERBS: [&str; 6] = ["get", "list", "create", "drop", "lease", "sync"];

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let Some(first) = args.first().map(String::as_str) else {
        let _ = out.write_all(HELP.as_bytes());
        return Exit::Ok;
    };
    match first {
        "-h" | "--help" | "help" => {
            let _ = out.write_all(HELP.as_bytes());
            Exit::Ok
        }
        "-V" | "--version" => {
            let _ = writeln!(out, "jerk {}", crate::version());
            Exit::Ok
        }
        "list" | "config" | "intercept" => not_yet(first, err),
        flag if flag.starts_with('-') => usage(&format!("unknown option {flag}"), err),
        provider => match args.get(1).map(String::as_str) {
            Some(verb) if VERBS.contains(&verb) => not_yet(&format!("{provider} {verb}"), err),
            Some(verb) => usage(&format!("unknown command {verb}"), err),
            None => usage(&format!("{provider}: missing a command"), err),
        },
    }
}

fn not_yet(what: &str, err: &mut dyn Write) -> Exit {
    let _ = writeln!(err, "jerk {what}: not implemented yet");
    Exit::Unsupported
}

fn usage(message: &str, err: &mut dyn Write) -> Exit {
    let _ = writeln!(err, "jerk: {message}\nRun jerk --help for usage.");
    Exit::Usage
}
