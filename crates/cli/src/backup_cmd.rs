//! `sherwood backup` / `sherwood restore` — copy the two pieces of durable
//! local state (the SQLite state database and the encrypted secret vault) to
//! and from a backup directory.
//!
//! Run these with `serve` / `run` **stopped**. SQLite in WAL mode keeps
//! `-wal` / `-shm` sidecars; a copy taken mid-write can be torn.
//!
//! `restore` overwrites live files. It refuses to clobber an existing target
//! unless `--force`, and always prints exactly which files it writes.

use crate::config::AppConfig;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const VAULT_ENV: &str = "SHERWOOD_VAULT_PATH";
const DEFAULT_VAULT: &str = "secrets.vault";

fn vault_path() -> PathBuf {
    std::env::var_os(VAULT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| DEFAULT_VAULT.into())
}

/// The state DB plus its WAL sidecars, and the vault — whatever exists.
fn state_files(cfg: &AppConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(db) = &cfg.general.state_path {
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{suffix}", db.display()));
            if p.is_file() {
                files.push(p);
            }
        }
    }
    let v = vault_path();
    if v.is_file() {
        files.push(v);
    }
    files
}

fn copy_into(src: &Path, dir: &Path) -> Result<u64> {
    let name = src
        .file_name()
        .with_context(|| format!("{} has no file name", src.display()))?;
    let dst = dir.join(name);
    let bytes = std::fs::copy(src, &dst)
        .with_context(|| format!("copying {} -> {}", src.display(), dst.display()))?;
    Ok(bytes)
}

pub fn backup(cfg: &AppConfig, dest: &Path) -> Result<()> {
    let files = state_files(cfg);
    if files.is_empty() {
        bail!(
            "nothing to back up — no [general] state_path database and no vault at {}",
            vault_path().display()
        );
    }

    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let out = dest.join(format!("sherwood-backup-{stamp}"));
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;

    let mut manifest = format!("sherwood backup {stamp}\n");
    for src in &files {
        let bytes = copy_into(src, &out)?;
        let line = format!("{}  {} bytes\n", src.display(), bytes);
        print!("  backed up {line}");
        manifest.push_str(&line);
    }
    std::fs::write(out.join("MANIFEST.txt"), manifest)?;
    println!("backup written to {}", out.display());
    Ok(())
}

pub fn restore(cfg: &AppConfig, from: &Path, force: bool) -> Result<()> {
    if !from.is_dir() {
        bail!("{} is not a backup directory", from.display());
    }

    // Map each file in the backup to where it belongs.
    let mut plan: Vec<(PathBuf, PathBuf)> = Vec::new();
    let vault = vault_path();
    for entry in std::fs::read_dir(from)? {
        let src = entry?.path();
        let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "MANIFEST.txt" || !src.is_file() {
            continue;
        }
        let target = if name == vault.file_name().and_then(|n| n.to_str()).unwrap_or("") {
            vault.clone()
        } else if let Some(db) = &cfg.general.state_path {
            let base = db.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == base || name == format!("{base}-wal") || name == format!("{base}-shm") {
                db.with_file_name(name)
            } else {
                tracing::warn!(file = name, "restore: no target for this file — skipping");
                continue;
            }
        } else {
            tracing::warn!(
                file = name,
                "restore: no [general] state_path — skipping DB file"
            );
            continue;
        };
        plan.push((src, target));
    }

    if plan.is_empty() {
        bail!("nothing in {} maps to this config", from.display());
    }

    let existing: Vec<&PathBuf> = plan.iter().map(|(_, t)| t).filter(|t| t.exists()).collect();
    if !existing.is_empty() && !force {
        for t in &existing {
            eprintln!("  would overwrite {}", t.display());
        }
        bail!("refusing to overwrite existing files — re-run with --force to proceed");
    }

    for (src, target) in &plan {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(src, target)
            .with_context(|| format!("restoring {} -> {}", src.display(), target.display()))?;
        println!("  restored {}", target.display());
    }
    println!("restore complete — start `sherwood serve` / `run` again");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn cfg_with_db(db: PathBuf) -> AppConfig {
        AppConfig {
            general: crate::config::General {
                starting_cash: dec!(1000),
                mode: "paper".into(),
                state_path: Some(db),
                feed_path: None,
                decider: "rule".into(),
            },
            risk: Default::default(),
            ai: Default::default(),
            copytrade: Default::default(),
            sniper: Default::default(),
            server: Default::default(),
            hook: Default::default(),
        }
    }

    #[test]
    fn backup_then_restore_round_trips_the_db() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        std::fs::write(&db, b"DB-CONTENTS-V1").unwrap();
        let cfg = cfg_with_db(db.clone());

        let dest = dir.path().join("backups");
        backup(&cfg, &dest).unwrap();

        // find the timestamped folder
        let bdir = std::fs::read_dir(&dest)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(bdir.join("MANIFEST.txt").is_file());
        assert!(bdir.join("state.db").is_file());

        // mutate the live DB, then restore over it
        std::fs::write(&db, b"CORRUPTED").unwrap();
        let err = restore(&cfg, &bdir, false).unwrap_err();
        assert!(err.to_string().contains("--force"));

        restore(&cfg, &bdir, true).unwrap();
        assert_eq!(std::fs::read(&db).unwrap(), b"DB-CONTENTS-V1");
    }

    #[test]
    fn backup_bails_when_there_is_nothing_to_copy() {
        // Point the vault at an absent path via a scoped guard (other tests in
        // this binary read `$SHERWOOD_VAULT_PATH` too).
        struct Guard(Option<std::ffi::OsString>);
        impl Drop for Guard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(v) => std::env::set_var(VAULT_ENV, v),
                    None => std::env::remove_var(VAULT_ENV),
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let _g = Guard(std::env::var_os(VAULT_ENV));
        std::env::set_var(VAULT_ENV, dir.path().join("absent.vault"));

        let cfg = cfg_with_db(dir.path().join("absent.db"));
        assert!(state_files(&cfg).is_empty());
        let err = backup(&cfg, &dir.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("nothing to back up"), "{err}");
    }
}
