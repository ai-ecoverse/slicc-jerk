use crate::paths::{Env, Os};
use std::path::{Path, PathBuf};

pub fn find(command: &str, env: &Env, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    if command.contains('/') || command.contains('\\') {
        let path = PathBuf::from(command);
        return exists(&path).then_some(path);
    }
    let separator = if env.os == Os::Windows { ';' } else { ':' };
    let extensions: Vec<String> = if env.os == Os::Windows {
        env.var("PATHEXT")
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };
    let path = env.var("PATH")?;
    for dir in path.split(separator).filter(|d| !d.is_empty()) {
        for ext in &extensions {
            let candidate = Path::new(dir).join(format!("{command}{ext}"));
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn is_file(path: &Path) -> bool {
    path.is_file()
}
