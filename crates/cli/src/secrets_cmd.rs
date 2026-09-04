//! `sherwood secrets` — manage the encrypted secret vault.
//!
//! The vault file is XChaCha20-Poly1305 over an Argon2id-derived key; the
//! passphrase comes from `$SHERWOOD_VAULT_PASSPHRASE`. Values are read from
//! stdin, never from the command line (argv is visible in `ps`).

use anyhow::{bail, Context, Result};
use sherwood_secrets::{FileVault, SecretsVault, DEFAULT_PASSPHRASE_ENV};
use std::io::Read;
use std::path::PathBuf;

const DEFAULT_VAULT: &str = "secrets.vault";

pub fn usage() -> ! {
    eprintln!(
        "sherwood secrets — manage the encrypted vault (needs $SHERWOOD_VAULT_PASSPHRASE)\n\n\
         USAGE:\n  \
         sherwood secrets set <name>    read the value from stdin and store it\n  \
         sherwood secrets get <name>    print the value to stdout (plaintext!)\n  \
         sherwood secrets list          list secret names\n  \
         sherwood secrets rm <name>     delete a secret\n\n\
         The vault file defaults to ./{DEFAULT_VAULT}; set SHERWOOD_VAULT_PATH to change it."
    );
    std::process::exit(2);
}

/// Open the vault at `$SHERWOOD_VAULT_PATH` (default `./secrets.vault`) using
/// the passphrase in `$SHERWOOD_VAULT_PASSPHRASE`. Shared with the runner, which
/// needs it to resolve `vault:` references in the config.
pub fn open_vault() -> Result<FileVault> {
    let path: PathBuf = std::env::var_os("SHERWOOD_VAULT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| DEFAULT_VAULT.into());
    FileVault::open(path, DEFAULT_PASSPHRASE_ENV).context("opening the vault")
}

fn open() -> Result<FileVault> {
    open_vault()
}

/// `args` is everything after `secrets`.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        Some("set") => {
            let name = args.next().unwrap_or_else(|| usage());
            let mut value = String::new();
            std::io::stdin()
                .read_to_string(&mut value)
                .context("reading the value from stdin")?;
            let value = value.trim_end_matches(['\n', '\r']);
            if value.is_empty() {
                bail!("no value on stdin");
            }
            open()?.set(&name, value)?;
            eprintln!("stored {name}");
            Ok(())
        }
        Some("get") => {
            let name = args.next().unwrap_or_else(|| usage());
            match open()?.get(&name)? {
                Some(s) => {
                    println!("{}", s.expose());
                    Ok(())
                }
                None => bail!("no secret named {name}"),
            }
        }
        Some("list") => {
            for name in open()?.list()? {
                println!("{name}");
            }
            Ok(())
        }
        Some("rm") => {
            let name = args.next().unwrap_or_else(|| usage());
            if open()?.delete(&name)? {
                eprintln!("removed {name}");
            } else {
                eprintln!("no secret named {name}");
            }
            Ok(())
        }
        _ => usage(),
    }
}
