use super::Config;
pub use crate::file_stamp::FileStamp;
use anyhow::Result;
use std::path::Path;

pub type ConfigFileStamp = (FileStamp, Option<FileStamp>);
pub fn config_stamp(path: &Path) -> Option<ConfigFileStamp> {
    Some((
        FileStamp::of_followed(path)?,
        path.parent()
            .and_then(|parent| FileStamp::of_followed(&parent.join("env"))),
    ))
}

#[derive(Default)]
pub struct ConfigCache {
    cached: std::sync::Mutex<Option<(std::path::PathBuf, ConfigFileStamp, Config)>>,
}

impl ConfigCache {
    pub fn load(&self, config_path: &Path) -> Result<Config> {
        let mut cached = self
            .cached
            .lock()
            .map_err(|_| anyhow::anyhow!("configuration cache unavailable"))?;
        let stamp = config_stamp(config_path);
        if let (Some(current_stamp), Some((cached_path, cached_stamp, cached_cfg))) =
            (&stamp, &*cached)
        {
            if config_path == cached_path && current_stamp == cached_stamp {
                return Ok(cached_cfg.clone());
            }
        }
        let loaded = Config::load_runtime(config_path)?;
        // A replacement during loading must not label the loaded value with a
        // different file's identity. The next scan will load it normally.
        *cached = stamp
            .filter(|before| Some(before) == config_stamp(config_path).as_ref())
            .map(|stamp| (config_path.into(), stamp, loaded.clone()));
        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn daemon_config_and_env_atomic_replacements_invalidate_same_size_and_mtime() {
        let root =
            std::env::temp_dir().join(format!("agenthub-daemon-stamp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        let env = root.join("env");
        let mut cfg = Config::current_defaults().unwrap();
        cfg.security.secret_key = crate::crypto::SecretBox::random_key_material();
        cfg.probe.poll_seconds = 25;
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        let cache = ConfigCache::default();
        assert_eq!(cache.load(&path).unwrap().probe.poll_seconds, 25);
        let replace = |path: &Path, text: &str| {
            let before = std::fs::metadata(path).unwrap();
            let next = path.with_extension("replacement");
            std::fs::write(&next, text).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&next)
                .unwrap()
                .set_modified(before.modified().unwrap())
                .unwrap();
            assert_eq!(std::fs::metadata(&next).unwrap().len(), before.len());
            std::fs::rename(next, path).unwrap();
        };
        cfg.probe.poll_seconds = 45;
        replace(&path, &toml::to_string(&cfg).unwrap());
        assert_eq!(cache.load(&path).unwrap().probe.poll_seconds, 45);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        cfg.probe.poll_seconds = 65;
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(cache.load(&path).unwrap().probe.poll_seconds, 65);
        // Creation is part of the cache identity even when TOML has precedence.
        let first = crate::crypto::SecretBox::random_key_material();
        let second = crate::crypto::SecretBox::random_key_material();
        std::fs::write(&env, format!("AGENTHUB_SECRET_KEY={first}\n")).unwrap();
        assert!(cache.load(&path).unwrap().security.secret_key == cfg.security.secret_key);
        assert!(
            cache
                .cached
                .lock()
                .unwrap()
                .as_ref()
                .map(|(_, stamp, _)| stamp)
                == config_stamp(&path).as_ref()
        );
        cfg.security.secret_key.clear();
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        assert!(cache.load(&path).unwrap().security.secret_key == first);
        replace(&env, &format!("AGENTHUB_SECRET_KEY={second}\n"));
        assert!(cache.load(&path).unwrap().security.secret_key == second);
        std::fs::remove_file(&env).unwrap();
        assert!(cache.load(&path).unwrap().security.secret_key != second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
