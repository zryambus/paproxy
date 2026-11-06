use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::cfg::{Cfg, get_config};

/// Name of the profile stored in the main config file
pub const DEFAULT: &str = "Default";

/// File in the profiles directory with the name of the last selected profile
const LAST_FILE: &str = ".last";

/// Named proxy settings: the main config file is the default profile, others are
/// files in the same format in the `profiles` directory next to it
pub struct Profiles {
    config_path: PathBuf,
    dir: PathBuf,
}

impl Profiles {
    pub fn new(config_path: PathBuf) -> Self {
        let dir = config_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("profiles");
        Self { config_path, dir }
    }

    /// The default profile first, then the others sorted by name
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let is_yaml = path.extension().is_some_and(|ext| ext == "yml");
                let name = path.file_stem()?.to_str()?.to_owned();
                (is_yaml && validate_name(&name).is_ok()).then_some(name)
            })
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.insert(0, DEFAULT.to_owned());
        names
    }

    pub fn path(&self, name: &str) -> PathBuf {
        if name == DEFAULT {
            self.config_path.clone()
        } else {
            self.dir.join(format!("{}.yml", name))
        }
    }

    pub fn load(&self, name: &str) -> anyhow::Result<Cfg> {
        get_config(Some(self.path(name)))
            .with_context(|| format!("Failed to load profile {}", name))
    }

    /// Writes the profile, comments of an existing file are kept
    pub fn save(&self, name: &str, cfg: &Cfg) -> anyhow::Result<()> {
        if name != DEFAULT {
            validate_name(name)?;
            std::fs::create_dir_all(&self.dir)
                .with_context(|| format!("Failed to create {}", self.dir.display()))?;
        }
        let path = self.path(name);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, cfg.update_yaml(&existing))
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn delete(&self, name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(name != DEFAULT, "The default profile cannot be deleted");
        let path = self.path(name);
        std::fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))
    }

    /// The last selected profile if it still exists
    pub fn last(&self) -> Option<String> {
        let name = std::fs::read_to_string(self.dir.join(LAST_FILE)).ok()?;
        let name = name.trim();
        (name == DEFAULT || (validate_name(name).is_ok() && self.path(name).exists()))
            .then(|| name.to_owned())
    }

    pub fn set_last(&self, name: &str) {
        let path = self.dir.join(LAST_FILE);
        // The default profile does not need the directory
        if name == DEFAULT && !self.dir.exists() {
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&self.dir).and_then(|_| std::fs::write(&path, name))
        {
            tracing::warn!("Failed to write {}: {}", path.display(), e);
        }
    }
}

/// Profile names become file names, so they must be valid on every platform
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "Profile name must not be empty");
    anyhow::ensure!(
        name.trim() == name,
        "Profile name must not start or end with spaces"
    );
    anyhow::ensure!(
        !name.eq_ignore_ascii_case(DEFAULT),
        "{} is the name of the main config file",
        DEFAULT
    );
    anyhow::ensure!(
        !name.starts_with('.') && !name.ends_with('.'),
        "Profile name must not start or end with a dot"
    );
    anyhow::ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c)),
        r#"Profile name must not contain / \ : * ? " < > |"#
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_profiles(test: &str) -> Profiles {
        let dir = std::env::temp_dir().join(format!("paproxy-{}-{}", test, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Profiles::new(dir.join("config.yml"))
    }

    fn cfg(host: &str) -> Cfg {
        Cfg {
            host: host.into(),
            ..Cfg::default()
        }
    }

    #[test]
    fn profiles_are_saved_listed_and_deleted() {
        let profiles = temp_profiles("crud");
        assert_eq!(profiles.list(), [DEFAULT]);

        profiles.save(DEFAULT, &cfg("default:5043")).unwrap();
        profiles.save("stage", &cfg("stage:5043")).unwrap();
        profiles.save("Alpha", &cfg("alpha:5043")).unwrap();

        assert_eq!(profiles.list(), [DEFAULT, "Alpha", "stage"]);
        assert_eq!(profiles.load(DEFAULT).unwrap().host, "default:5043");
        assert_eq!(profiles.load("stage").unwrap().host, "stage:5043");
        assert!(profiles.path(DEFAULT).ends_with("config.yml"));

        profiles.delete("stage").unwrap();
        assert_eq!(profiles.list(), [DEFAULT, "Alpha"]);
        assert!(profiles.delete(DEFAULT).is_err());
    }

    #[test]
    fn last_profile_is_remembered() {
        let profiles = temp_profiles("last");
        assert_eq!(profiles.last(), None);

        profiles.save("alpha", &cfg("alpha:5043")).unwrap();
        profiles.set_last("alpha");
        assert_eq!(profiles.last().as_deref(), Some("alpha"));

        // A deleted profile is forgotten
        profiles.delete("alpha").unwrap();
        assert_eq!(profiles.last(), None);
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in [
            "", " ", " a", "a/b", r"a\b", "a:b", ".hidden", "a.", "default",
        ] {
            assert!(validate_name(name).is_err(), "{:?}", name);
        }
        for name in ["alpha", "Main server", "pa6-1.2"] {
            assert!(validate_name(name).is_ok(), "{:?}", name);
        }
        assert!(temp_profiles("invalid").save("a/b", &cfg("a:1")).is_err());
    }
}
