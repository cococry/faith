use std::path::PathBuf;

pub fn directory() -> anyhow::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Executable has no parent directory"))?
        .join("../share/faith/assets");

    if directory.is_dir() {
        return Ok(directory);
    }

    #[cfg(debug_assertions)]
    {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

        if directory.is_dir() {
            return Ok(directory);
        }
    }

    anyhow::bail!(
        "Faith assets were not found. Run the packaged AppImage or install assets in share/faith/assets next to the bin directory."
    )
}
