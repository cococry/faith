use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
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
