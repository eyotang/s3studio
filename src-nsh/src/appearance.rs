use nsh_core::prelude::{Appearance, set_color_scheme};
use std::{io, path::PathBuf};

fn path() -> io::Result<PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("com.s3studio.desktop").join("theme"))
        .ok_or_else(|| io::Error::other("Unable to locate settings directory"))
}

pub fn preference() -> Appearance {
    match path()
        .and_then(std::fs::read_to_string)
        .unwrap_or_default()
        .trim()
    {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        _ => Appearance::System,
    }
}

pub fn select(value: Appearance) -> io::Result<()> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let code = match value {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
        Appearance::System => "system",
    };
    std::fs::write(&temporary, code)?;
    std::fs::rename(temporary, path)?;
    set_color_scheme(value);
    Ok(())
}
