//! Wires the local `config.toml` file into
//! `sherwood_server::state::ConfigStore` for `sherwood serve`
//! (`GET`/`POST /v1/config`, v0.2.13).

use sherwood_server::state::ConfigStore;
use std::path::PathBuf;

pub struct FileConfigStore {
    pub path: PathBuf,
}

impl ConfigStore for FileConfigStore {
    fn read(&self) -> Result<String, String> {
        std::fs::read_to_string(&self.path)
            .map_err(|e| format!("reading {}: {e}", self.path.display()))
    }

    /// Writes atomically: a temp file in the same directory, then an
    /// OS-level rename — a crash mid-write can't leave `config.toml`
    /// truncated or half-written.
    fn write(&self, contents: &str) -> Result<(), String> {
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, contents).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("renaming {} to {}: {e}", tmp.display(), self.path.display()))
    }
}
