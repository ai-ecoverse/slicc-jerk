use crate::cli::{Ctx, Io};
use crate::config::Entry;
use crate::exit::Exit;
use crate::paths::Os;
use crate::run::{Opts, Outcome};
use crate::which;

const SH: &str = "trap 'stty echo' EXIT INT TERM; stty -echo; IFS= read -r key; printf %s \"$key\"";
const PS: &str = "$k = Read-Host -AsSecureString; [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($k))";

pub fn reader(os: Os) -> (&'static str, Vec<String>) {
    match os {
        Os::Windows => (
            "powershell",
            vec!["-NoProfile".into(), "-Command".into(), PS.into()],
        ),
        _ => ("sh", vec!["-c".into(), SH.into()]),
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

fn hidden(entry: &Entry, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<String, Exit> {
    let (program, args) = reader(ctx.env.os);
    let Some(path) = which::find(program, ctx.env, ctx.exists) else {
        let _ = writeln!(
            io.err,
            "jerk: {}: {program} is not on PATH, so pipe the key in",
            entry.id
        );
        return Err(Exit::Unavailable);
    };
    let _ = write!(io.err, "Paste the key (it won't show): ");
    let _ = io.err.flush();
    let opts = Opts {
        interactive: true,
        capture: true,
        echo: false,
        timeout: None,
        remove_env: &[],
        input: None,
        on_stderr: None,
    };
    let outcome = ctx.runner.run(&path, &args, &opts);
    let _ = writeln!(io.err);
    match outcome {
        Outcome::Exited {
            code: 0, stdout, ..
        } => Ok(stdout),
        _ => {
            let _ = writeln!(io.err, "jerk: {}: cannot read the key", entry.id);
            Err(Exit::Error)
        }
    }
}

pub fn ask(entry: &Entry, url: &str, ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Result<String, Exit> {
    let _ = writeln!(io.err, "jerk: {}: create a key at {url}", entry.id);
    let text = if ctx.stdin_terminal {
        let _ = ctx.oauth.browser().open_url(url);
        hidden(entry, ctx, io)?
    } else {
        let mut text = String::new();
        let _ = ctx.input.read_to_string(&mut text);
        text
    };
    first_line(&text).ok_or_else(|| {
        let _ = writeln!(
            io.err,
            "jerk: {}: no key given; paste it on a terminal or pipe it in",
            entry.id
        );
        Exit::SignIn
    })
}
