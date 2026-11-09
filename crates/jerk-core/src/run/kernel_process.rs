use super::{Opts, Outcome, Process, Runner};
use serde_json::json;
use std::path::Path;
use zeroize::Zeroizing;

#[link(wasm_import_module = "jerk_process")]
extern "C" {
    fn run(ptr: *const u8, len: usize) -> usize;
    fn take(ptr: *mut u8, len: usize) -> u32;
}

impl Runner for Process {
    fn run(&self, program: &Path, args: &[String], opts: &Opts<'_>) -> Outcome {
        let argv: Vec<String> = std::iter::once(program.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect();
        let request = Zeroizing::new(
            json!({
                "argv": argv,
                "remove_env": opts.remove_env,
                "input": opts.input,
                "interactive": opts.interactive,
                "timeout_ms": opts.timeout.map(|t| t.as_millis() as u64),
            })
            .to_string(),
        );
        let len = unsafe { run(request.as_ptr(), request.len()) };
        if len > 16 * 1024 * 1024 + 4096 {
            return Outcome::Failed("invalid kernel process response".into());
        }
        let mut bytes = Zeroizing::new(vec![0; len]);
        if unsafe { take(bytes.as_mut_ptr(), len) } != 0 {
            return Outcome::Failed("kernel process imports are unavailable".into());
        }
        let Ok(reply) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Outcome::Failed("invalid kernel process response".into());
        };
        if reply["timeout"] == true {
            return Outcome::TimedOut;
        }
        if let Some(code) = reply["code"].as_i64() {
            return Outcome::Exited {
                code: code as i32,
                stdout: reply["stdout"].as_str().unwrap_or_default().to_string(),
                stderr: reply["stderr"].as_str().unwrap_or_default().to_string(),
            };
        }
        Outcome::Failed("cannot run provider in the kernel".into())
    }
}
