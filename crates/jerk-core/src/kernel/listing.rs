use crate::cli::{usage, Ctx, Io};
use crate::config::Entry;
use crate::dispatch;
use crate::exit::Exit;
use crate::sealed::Sealed;
use serde_json::{json, Value};

fn records(entry: &Entry, sealed: &dyn Sealed) -> Result<Vec<Value>, ()> {
    let reply = sealed
        .request(&json!({"v": 1, "op": "list"}))
        .map_err(|_| ())?;
    if reply.get("error").is_some() {
        return Err(());
    }
    let records = reply["secrets"].as_array().ok_or(())?;
    Ok(records
        .iter()
        .filter(|record| {
            record["name"]
                .as_str()
                .is_some_and(|name| entry.provider.env.iter().any(|n| n == name))
                && record["state"] != "removed"
        })
        .map(|record| {
            let mut item = serde_json::Map::new();
            for key in ["name", "placeholder", "state", "exec", "rules", "expiresAt"] {
                if let Some(value) = record.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            item.insert("source".into(), json!("sealed"));
            Value::Object(item)
        })
        .collect())
}

pub fn run(
    entry: &Entry,
    args: &[String],
    ctx: &mut Ctx<'_>,
    sealed: &dyn Sealed,
    io: &mut Io<'_>,
) -> Exit {
    let flags = match dispatch::parse_flags(args, &[], &["--json"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} list: {e}", entry.id), io),
    };
    if let Err(e) = dispatch::vars(entry, &flags) {
        return usage(&e, io);
    }
    let vault = records(entry, sealed);
    let mut items = vault.as_ref().cloned().unwrap_or_default();
    let mut out = Vec::new();
    let mut err = Vec::new();
    if entry.provider.list.is_some() {
        let mut cli_args = args.to_vec();
        if !items.is_empty() && !flags.has("--json") {
            cli_args.push("--json".into());
        }
        let code = dispatch::list(
            entry,
            &cli_args,
            ctx,
            &mut Io {
                out: &mut out,
                err: &mut err,
            },
        );
        if code == Exit::Ok {
            if items.is_empty() {
                let _ = io.out.write_all(&out);
                return Exit::Ok;
            }
            let mut doc: Value = serde_json::from_slice(&out).expect("list emits JSON");
            let cli_items = doc["items"].as_array_mut().expect("list emits items");
            cli_items.append(&mut items);
            items = std::mem::take(cli_items);
        } else if items.is_empty() {
            let _ = io.err.write_all(&err);
            return code;
        }
    } else if vault.is_err() {
        let _ = writeln!(io.err, "jerk: {}: cannot list sealed records", entry.id);
        return Exit::Unavailable;
    }
    let text = if flags.has("--json") {
        json!({"items": items}).to_string()
    } else {
        serde_json::to_string_pretty(&items).unwrap_or_default()
    };
    let _ = writeln!(io.out, "{text}");
    Exit::Ok
}
