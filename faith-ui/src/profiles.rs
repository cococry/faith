use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Profile {
    pub alias: String,
    pub path: PathBuf,
}

pub fn root() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(path.join("faith/profiles"));
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| anyhow::anyhow!("HOME must be an absolute path"))?;
    Ok(home.join(".local/share/faith/profiles"))
}

pub fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 20
        && alias
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

pub fn discover(root: &Path) -> anyhow::Result<Vec<Profile>> {
    if !root.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
    }

    let metadata = fs::symlink_metadata(root)?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { nix::libc::geteuid() },
        "profiles directory must be owned by the current user"
    );
    let mut profiles = Vec::new();

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let Some(alias) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };

        if entry.file_type()?.is_dir()
            && !entry.path().join(".username-pending").exists()
            && !alias.is_empty()
            && alias
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            profiles.push(Profile {
                alias,
                path: entry.path(),
            });
        }
    }

    profiles.sort_by(|a, b| a.alias.cmp(&b.alias));
    Ok(profiles)
}

pub fn create(root: &Path, alias: &str) -> anyhow::Result<Profile> {
    anyhow::ensure!(
        valid_alias(alias),
        "Use 1–20 letters, numbers, or underscores."
    );
    let path = root.join(alias);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow::anyhow!("A profile named {alias} already exists.")
            } else {
                e.into()
            }
        })?;

    Ok(Profile {
        alias: alias.to_owned(),
        path,
    })
}

pub fn delete(root: &Path, profile: &Profile) -> anyhow::Result<Option<std::io::Error>> {
    anyhow::ensure!(
        profile.path.parent() == Some(root),
        "Profile is outside the profiles directory"
    );
    let metadata = fs::symlink_metadata(&profile.path)?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { nix::libc::geteuid() },
        "Profile must be an owned directory"
    );
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
        .open(profile.path.join("profile.lock"))?;
    let metadata = lock.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.uid() == unsafe { nix::libc::geteuid() },
        "Profile lock must be an owned regular file"
    );
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("Profile is open in another Faith session"))?;

    let staging = root.join(format!(".deleted-{}", faith_client::Id::random()?));
    fs::DirBuilder::new().mode(0o700).create(&staging)?;
    if let Err(error) = fs::rename(&profile.path, staging.join("profile")) {
        let _ = fs::remove_dir(&staging);
        return Err(error.into());
    }

    Ok(fs::remove_dir_all(staging).err())
}

pub const USERNAME_MAX: usize = 20;
pub const USERNAME_ERROR: &str = "Use 1–20 ASCII letters, digits 1–9, or underscores.";

pub fn username_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '1'..='9' | '_')
}

pub fn valid_username(name: &str) -> bool {
    !name.is_empty() && name.len() <= USERNAME_MAX && name.chars().all(username_char)
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct UsernameSettings {
    pub username: Option<String>,
    pub auto_disclose: bool,
    pub reject_required: bool,
}

impl UsernameSettings {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path.join("username.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

pub fn check_claim_alias(root: &Path, alias: &str) -> anyhow::Result<Option<Profile>> {
    anyhow::ensure!(
        valid_alias(alias),
        "Use 1–20 letters, numbers, or underscores."
    );
    let path = root.join(alias);

    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir()
                    && path.join(".username-pending").is_file()
                    && !path.join("username.json").exists(),
                "A profile named {alias} already exists."
            );
            Ok(Some(Profile {
                alias: alias.to_owned(),
                path,
            }))
        }

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn prepare_claim(root: &Path, alias: &str) -> anyhow::Result<Profile> {
    if let Some(profile) = check_claim_alias(root, alias)? {
        return Ok(profile);
    }

    let profile = create(root, alias)?;
    fs::File::create(profile.path.join(".username-pending"))?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_respects_profile_locks_and_leaves_other_profiles() -> anyhow::Result<()> {
        let root =
            std::env::temp_dir().join(format!("faith-delete-{}", faith_client::Id::random()?));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let result = (|| -> anyhow::Result<()> {
            let home = create(&root, "Home")?;
            let work = create(&root, "Work")?;
            fs::write(home.path.join("identity.bin"), b"identity")?;
            let lock = fs::File::create(home.path.join("profile.lock"))?;
            lock.try_lock()?;
            assert!(delete(&root, &home).is_err());
            assert!(home.path.join("identity.bin").is_file());
            drop(lock);

            assert!(delete(&root, &home)?.is_none());
            assert!(!home.path.exists());
            assert!(work.path.is_dir());
            assert_eq!(fs::read_dir(&root)?.count(), 1);
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(&root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    fn username_character_rules_match_the_protocol() {
        assert!(valid_username("Faith_123456789"));
        assert!(valid_username(&"A".repeat(USERNAME_MAX)));

        for invalid in ["", "Faith0", "Faith-1", " Faith", "é", "A\0"] {
            assert!(!valid_username(invalid), "{invalid:?}");
        }

        assert!(!valid_username(&"A".repeat(USERNAME_MAX + 1)));
    }
}
