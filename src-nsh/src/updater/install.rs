use std::{fs, io, io::Read};
use tempfile::NamedTempFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Package {
    Nsis,
    Msi,
    AppImage,
}

impl Package {
    pub(super) fn installed() -> Option<Self> {
        if cfg!(all(windows, target_arch = "x86_64")) {
            let marker = std::env::current_exe()
                .ok()?
                .with_file_name("s3-studio.install");
            let mut kind = String::new();
            fs::File::open(marker)
                .ok()?
                .take(16)
                .read_to_string(&mut kind)
                .ok()?;
            match kind.trim() {
                "nsis" => Some(Self::Nsis),
                "msi" => Some(Self::Msi),
                _ => None,
            }
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            std::env::var_os("APPIMAGE").map(|_| Self::AppImage)
        } else {
            None
        }
    }

    pub(super) fn manifest_key(self) -> &'static str {
        match self {
            Self::Nsis => "windows-x86_64-nsis",
            Self::Msi => "windows-x86_64-msi",
            Self::AppImage => "linux-x86_64-appimage",
        }
    }

    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Nsis => ".exe",
            Self::Msi => ".msi",
            Self::AppImage => ".AppImage",
        }
    }
}

pub(super) struct StagedUpdate {
    pub(super) file: NamedTempFile,
    #[cfg(windows)]
    package: Package,
    #[cfg(target_os = "linux")]
    destination: std::path::PathBuf,
    #[cfg(target_os = "linux")]
    original: fs::Metadata,
}

impl StagedUpdate {
    #[cfg(windows)]
    pub(super) fn new(package: Package) -> io::Result<Self> {
        Ok(Self {
            file: tempfile::Builder::new()
                .prefix("s3-studio-update-")
                .suffix(package.extension())
                .tempfile()?,
            package,
        })
    }

    #[cfg(windows)]
    pub(super) fn install(self) -> io::Result<()> {
        use std::{ffi::OsString, path::PathBuf, process::Command};

        let (file, installer) = self.file.keep()?;
        drop(file);
        let result = match self.package {
            Package::Nsis => Command::new(&installer)
                .args(["/P", "/UPDATE", "/R"])
                .spawn(),
            Package::Msi => {
                let msiexec = std::env::var_os("SystemRoot")
                    .map(|root| {
                        PathBuf::from(root)
                            .join("System32/msiexec.exe")
                            .into_os_string()
                    })
                    .unwrap_or_else(|| OsString::from("msiexec.exe"));
                Command::new(msiexec)
                    .arg("/i")
                    .arg(&installer)
                    .args(["/passive", "/promptrestart", "AUTOLAUNCHAPP=True"])
                    .spawn()
            }
            Package::AppImage => Err(io::Error::from(io::ErrorKind::Unsupported)),
        };
        match result {
            Ok(_) => std::process::exit(0),
            Err(error) => {
                let _ = fs::remove_file(installer);
                Err(error)
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn new(_package: Package) -> io::Result<Self> {
        let path =
            std::env::var_os("APPIMAGE").ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Self::appimage(std::path::Path::new(&path))
    }

    #[cfg(target_os = "linux")]
    fn appimage(path: &std::path::Path) -> io::Result<Self> {
        let destination = fs::canonicalize(path)?;
        let current = fs::File::open(&destination)?;
        let original = current.metadata()?;
        if !original.is_file() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        check_appimage(current)?;
        let file = tempfile::Builder::new()
            .prefix(".s3-studio-update-")
            .tempfile_in(destination.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
        Ok(Self {
            file,
            destination,
            original,
        })
    }

    #[cfg(target_os = "linux")]
    pub(super) fn install(self) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;

        check_appimage(self.file.reopen()?)?;
        let current = fs::symlink_metadata(&self.destination)?;
        if current.ino() != self.original.ino()
            || current.dev() != self.original.dev()
            || current.modified()? != self.original.modified()?
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "AppImage changed during download",
            ));
        }
        self.file
            .as_file()
            .set_permissions(self.original.permissions())?;
        self.file.persist(self.destination)?;
        Ok(())
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub(super) fn new(_package: Package) -> io::Result<Self> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub(super) fn install(self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(target_os = "linux")]
fn check_appimage(mut file: fs::File) -> io::Result<()> {
    let mut header = [0; 11];
    file.read_exact(&mut header)?;
    if &header[..4] == b"\x7fELF" && &header[8..] == b"AI\x02" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid AppImage header",
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn replaces_appimage_and_preserves_execute_bits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.AppImage");
        let old = b"\x7fELF\x00\x00\x00\x00AI\x02original";
        let new = b"\x7fELF\x00\x00\x00\x00AI\x02updated";
        fs::write(&path, old).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        let stage = StagedUpdate::appimage(&path).unwrap();
        fs::write(stage.file.path(), new).unwrap();
        assert_eq!(fs::read(&path).unwrap(), old);
        stage.install().unwrap();
        assert_eq!(fs::read(&path).unwrap(), new);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let stage = StagedUpdate::appimage(&path).unwrap();
        fs::write(stage.file.path(), new).unwrap();
        let replacement = directory.path().join("replacement");
        fs::write(&replacement, old).unwrap();
        fs::rename(replacement, &path).unwrap();
        assert!(stage.install().is_err());
        assert_eq!(fs::read(path).unwrap(), old);
    }
}
