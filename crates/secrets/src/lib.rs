//! A tiny encrypted secret store.
//!
//! Secrets — API keys, tokens — live in a [`FileVault`] on disk: an
//! Argon2id-derived key, XChaCha20-Poly1305 over a JSON map of name → value.
//! The passphrase comes from an environment variable the operator exports; it
//! is never written anywhere.
//!
//! Callers hold a [`SecretsVault`] and get [`SecretString`]s — a string that
//! zeroes on drop and prints as `[redacted]`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &str = "SHERWOOD-VAULT-1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;

/// The default environment variable the file vault reads its passphrase from.
pub const DEFAULT_PASSPHRASE_ENV: &str = "SHERWOOD_VAULT_PASSPHRASE";

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("environment variable {0} is not set — export the vault passphrase")]
    MissingPassphrase(String),
    #[error("vault file is malformed: {0}")]
    Malformed(String),
    #[error("could not decrypt the vault — wrong passphrase, or the file was tampered with")]
    Decrypt,
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A string that is zeroed when dropped and never prints its contents.
#[derive(Clone, Serialize, Deserialize, zeroize::Zeroize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// The underlying value. Handle it briefly and do not log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString([redacted])")
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        // Not constant-time; callers doing auth comparisons must use a
        // constant-time check on `expose()`.
        self.0 == other.0
    }
}

/// Read and write named secrets.
pub trait SecretsVault: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<SecretString>, VaultError>;
    fn set(&self, name: &str, value: &str) -> Result<(), VaultError>;
    /// Returns `true` if a secret was removed.
    fn delete(&self, name: &str) -> Result<bool, VaultError>;
    fn list(&self) -> Result<Vec<String>, VaultError>;
}

fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::default()
        .hash_password_into(passphrase, salt, key.as_mut_slice())
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    Ok(key)
}

/// A file-backed vault. The file holds an encrypted JSON map.
pub struct FileVault {
    path: PathBuf,
    passphrase: SecretString,
}

impl FileVault {
    /// Open (not necessarily existing) a vault at `path`, taking the passphrase
    /// from environment variable `env_var`.
    pub fn open(path: impl Into<PathBuf>, env_var: &str) -> Result<Self, VaultError> {
        let passphrase = std::env::var(env_var)
            .map_err(|_| VaultError::MissingPassphrase(env_var.to_string()))?;
        Ok(Self {
            path: path.into(),
            passphrase: SecretString::new(passphrase),
        })
    }

    fn io_err(&self, source: std::io::Error) -> VaultError {
        VaultError::Io {
            path: self.path.display().to_string(),
            source,
        }
    }

    fn load_map(&self) -> Result<BTreeMap<String, String>, VaultError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(self.io_err(e)),
        };
        let mut lines = raw.lines();
        if lines.next() != Some(MAGIC) {
            return Err(VaultError::Malformed("missing or wrong magic line".into()));
        }
        let mut field = |what: &str| -> Result<Vec<u8>, VaultError> {
            let line = lines
                .next()
                .ok_or_else(|| VaultError::Malformed(format!("missing {what} line")))?;
            hex::decode(line.trim())
                .map_err(|_| VaultError::Malformed(format!("{what} is not hex")))
        };
        let salt = field("salt")?;
        let nonce = field("nonce")?;
        let ciphertext = field("ciphertext")?;
        if salt.len() != SALT_LEN || nonce.len() != NONCE_LEN {
            return Err(VaultError::Malformed(
                "salt or nonce has the wrong length".into(),
            ));
        }
        let nonce: [u8; NONCE_LEN] = nonce
            .try_into()
            .map_err(|_| VaultError::Malformed("nonce length".into()))?;

        let key = derive_key(self.passphrase.expose().as_bytes(), &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
            .map_err(|e| VaultError::Kdf(e.to_string()))?;
        let mut plaintext = cipher
            .decrypt(&XNonce::from(nonce), ciphertext.as_ref())
            .map_err(|_| VaultError::Decrypt)?;
        let map: BTreeMap<String, String> = serde_json::from_slice(&plaintext)?;
        plaintext.zeroize();
        Ok(map)
    }

    fn store_map(&self, map: &BTreeMap<String, String>) -> Result<(), VaultError> {
        let mut plaintext = serde_json::to_vec(map)?;
        let mut salt = [0u8; SALT_LEN];
        // OsRng: getrandom-backed.
        use chacha20poly1305::aead::rand_core::RngCore;
        OsRng.fill_bytes(&mut salt);
        let key = derive_key(self.passphrase.expose().as_bytes(), &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
            .map_err(|e| VaultError::Kdf(e.to_string()))?;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| VaultError::Decrypt)?;
        plaintext.zeroize();

        let body = format!(
            "{MAGIC}\n{}\n{}\n{}\n",
            hex::encode(salt),
            hex::encode(nonce),
            hex::encode(ciphertext)
        );
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| self.io_err(e))?;
        }
        write_private(&self.path, body.as_bytes()).map_err(|e| self.io_err(e))
    }
}

/// Write `bytes` to `path`, `0600` on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

impl SecretsVault for FileVault {
    fn get(&self, name: &str) -> Result<Option<SecretString>, VaultError> {
        Ok(self
            .load_map()?
            .get(name)
            .map(|v| SecretString::new(v.clone())))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), VaultError> {
        let mut map = self.load_map()?;
        map.insert(name.to_string(), value.to_string());
        self.store_map(&map)
    }

    fn delete(&self, name: &str) -> Result<bool, VaultError> {
        let mut map = self.load_map()?;
        let removed = map.remove(name).is_some();
        if removed {
            self.store_map(&map)?;
        }
        Ok(removed)
    }

    fn list(&self) -> Result<Vec<String>, VaultError> {
        Ok(self.load_map()?.into_keys().collect())
    }
}

/// Resolve a possible `vault:NAME` reference against `vault`. A value that is
/// not a reference is returned as-is. Used to keep secret *values* out of
/// config files — the file holds `api_key = "vault:nvidia"`.
pub fn resolve_ref(
    value: &str,
    vault: &dyn SecretsVault,
) -> Result<Option<SecretString>, VaultError> {
    match value.strip_prefix("vault:") {
        Some(name) => vault.get(name.trim()),
        None => Ok(Some(SecretString::new(value))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard(&'static str);
    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            std::env::set_var(key, val);
            Self(key)
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    fn vault(dir: &tempfile::TempDir, env: &'static str) -> FileVault {
        FileVault::open(dir.path().join("v.vault"), env).unwrap()
    }

    #[test]
    fn set_get_list_delete_round_trip() {
        let _g = EnvGuard::set("SHERWOOD_TEST_VP_1", "correct horse battery staple");
        let dir = tempfile::tempdir().unwrap();
        let v = vault(&dir, "SHERWOOD_TEST_VP_1");

        assert_eq!(v.get("nvidia").unwrap(), None);
        v.set("nvidia", "nv-abc123").unwrap();
        v.set("groq", "gsk-xyz").unwrap();

        assert_eq!(v.get("nvidia").unwrap().unwrap().expose(), "nv-abc123");
        let mut keys = v.list().unwrap();
        keys.sort();
        assert_eq!(keys, ["groq", "nvidia"]);

        assert!(v.delete("groq").unwrap());
        assert!(!v.delete("groq").unwrap());
        assert_eq!(v.list().unwrap(), ["nvidia"]);
    }

    #[test]
    fn persists_across_reopen() {
        let _g = EnvGuard::set("SHERWOOD_TEST_VP_2", "hunter2hunter2");
        let dir = tempfile::tempdir().unwrap();
        vault(&dir, "SHERWOOD_TEST_VP_2").set("k", "v").unwrap();
        // fresh instance, same file + passphrase
        assert_eq!(
            vault(&dir, "SHERWOOD_TEST_VP_2")
                .get("k")
                .unwrap()
                .unwrap()
                .expose(),
            "v"
        );
    }

    #[test]
    fn wrong_passphrase_fails_to_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _g = EnvGuard::set("SHERWOOD_TEST_VP_3", "right");
            vault(&dir, "SHERWOOD_TEST_VP_3").set("k", "v").unwrap();
        }
        let _g = EnvGuard::set("SHERWOOD_TEST_VP_3", "wrong");
        assert!(matches!(
            vault(&dir, "SHERWOOD_TEST_VP_3").get("k"),
            Err(VaultError::Decrypt)
        ));
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let _g = EnvGuard::set("SHERWOOD_TEST_VP_4", "pw");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.vault");
        FileVault::open(&path, "SHERWOOD_TEST_VP_4")
            .unwrap()
            .set("k", "v")
            .unwrap();

        let mut lines: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        // flip a hex nibble in the ciphertext line
        let ct = lines.last_mut().unwrap();
        let first = &ct[..1];
        let flipped = if first == "a" { "b" } else { "a" };
        *ct = format!("{flipped}{}", &ct[1..]);
        std::fs::write(&path, lines.join("\n")).unwrap();

        assert!(matches!(
            FileVault::open(&path, "SHERWOOD_TEST_VP_4")
                .unwrap()
                .get("k"),
            Err(VaultError::Decrypt)
        ));
    }

    #[test]
    fn missing_passphrase_env_is_a_clear_error() {
        std::env::remove_var("SHERWOOD_TEST_VP_NONE");
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            FileVault::open(dir.path().join("v"), "SHERWOOD_TEST_VP_NONE"),
            Err(VaultError::MissingPassphrase(_))
        ));
    }

    #[test]
    fn resolve_ref_passes_plain_values_and_looks_up_refs() {
        let _g = EnvGuard::set("SHERWOOD_TEST_VP_5", "pw");
        let dir = tempfile::tempdir().unwrap();
        let v = vault(&dir, "SHERWOOD_TEST_VP_5");
        v.set("nvidia", "nv-key").unwrap();

        assert_eq!(
            resolve_ref("literal", &v).unwrap().unwrap().expose(),
            "literal"
        );
        assert_eq!(
            resolve_ref("vault:nvidia", &v).unwrap().unwrap().expose(),
            "nv-key"
        );
        assert_eq!(resolve_ref("vault:absent", &v).unwrap(), None);
    }

    #[test]
    fn secret_string_debug_is_redacted() {
        let s = SecretString::new("topsecret");
        assert_eq!(format!("{s:?}"), "SecretString([redacted])");
        assert!(!format!("{s:#?}").contains("topsecret"));
    }
}
