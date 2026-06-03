use std::path::PathBuf;

use rusqlite::{params, Connection, OpenFlags};

use crate::config::{CC_SWITCH_CODEX_PROVIDER_ID, ProviderConfig};
use crate::paths;

/// 将 Desktop 会话库里残留的 model_provider 同步为当前 config（避免 UI 仍显示旧状态）。
pub fn sync_threads_to_config(provider: &ProviderConfig) -> anyhow::Result<usize> {
    let target_provider = CC_SWITCH_CODEX_PROVIDER_ID;
    let target_model = provider.default_model.clone();
    let mut total = 0usize;

    for db in candidate_state_dbs()? {
        total += sync_one_db(&db, target_provider, &target_model)?;
    }
    Ok(total)
}

fn candidate_state_dbs() -> anyhow::Result<Vec<PathBuf>> {
    let home = paths::codex_home_dir()?;
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&home) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with("state_") && name.ends_with(".sqlite") {
            out.push(path);
        }
    }
    Ok(out)
}

fn sync_one_db(path: &PathBuf, provider: &str, model: &str) -> anyhow::Result<usize> {
    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };

    let changed = conn.execute(
        "UPDATE threads SET model_provider = ?1, model = ?2
         WHERE model_provider IS NOT ?1 OR model IS NOT ?2",
        params![provider, model],
    )?;
    Ok(changed)
}
