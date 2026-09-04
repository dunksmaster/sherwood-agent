//! `sherwood wallet-address <secret-name>` — derive and print a wallet's
//! **address only** from a private key stored in the vault.
//!
//! This never prints, logs, or returns the key itself — only the derived
//! address, which is safe to share (it's what you'd hand a faucet or use to
//! fund the wallet). It signs nothing and sends nothing; there is no
//! transaction path here yet (`sherwood-dex`, later).
//!
//! The key must already be in the vault: `sherwood secrets set <secret-name>`
//! (reads the hex private key from stdin, never argv).

use crate::secrets_cmd::open_vault;
use anyhow::{bail, Context, Result};
use sherwood_secrets::SecretsVault;
use sherwood_signer::LocalSigner;

pub fn usage() -> ! {
    eprintln!(
        "sherwood wallet-address <secret-name>\n\n\
         Derives and prints the wallet address for a private key already stored\n\
         in the vault (sherwood secrets set <secret-name>). Prints the address\n\
         only — never the key.\n"
    );
    std::process::exit(2);
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let name = args.next().unwrap_or_else(|| usage());
    let vault = open_vault()?;
    let secret = vault
        .get(&name)
        .context("reading the vault")?
        .with_context(|| {
            format!("no secret named {name} — run `sherwood secrets set {name}` first")
        })?;

    let signer = match LocalSigner::from_hex(&secret) {
        Ok(s) => s,
        Err(e) => bail!("{name} is not a valid secp256k1 private key: {e}"),
    };
    println!("{}", signer.address_hex());
    Ok(())
}
