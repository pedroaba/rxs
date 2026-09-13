use crate::capture::{CaptureArtifact, CaptureBackend, CaptureMode, CaptureOutcome};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::Command,
};

pub struct Session {
    pub root: PathBuf,
    _lock: File,
}

impl Session {
    pub fn open() -> Result<Self, String> {
        // A private root plus an advisory lock prevents one instance deleting
        // another instance's captures. Never follow a substituted root symlink.
        let uid = unsafe { libc::getuid() };
        let root = std::env::temp_dir().join(format!("app.rxs.capture-{uid}"));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Não foi possível criar a pasta temporária: {e}")),
        }
        let metadata = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(
                "A pasta temporária do RXS não é privada ou tem proprietário incorreto.".into(),
            );
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join("instance.lock"))
            .map_err(|e| e.to_string())?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("O RXS já está aberto. Use seu ícone na barra de menus.".into());
        }
        for entry in fs::read_dir(&root).map_err(|e| e.to_string())?.flatten() {
            if entry.file_name().to_string_lossy().starts_with("session-")
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                fs::remove_dir_all(entry.path())
                    .map_err(|e| format!("Falha ao limpar captura temporária anterior: {e}"))?;
            }
        }
        Ok(Self { root, _lock: lock })
    }
}

pub struct MacCapture {
    pub root: PathBuf,
}

impl CaptureBackend for MacCapture {
    fn capture(&self, mode: CaptureMode) -> CaptureOutcome {
        let directory = match tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(&self.root)
        {
            Ok(dir) => dir,
            Err(e) => {
                return CaptureOutcome::Error(format!("Não foi possível preparar a captura: {e}"));
            }
        };
        let path = directory.path().join("capture.png");
        let mut command = Command::new("/usr/sbin/screencapture");
        // Let macOS play its shutter feedback only when a capture is taken.
        command.args(["-t", "png"]);
        match mode {
            CaptureMode::Screen => {
                command.arg("-m");
            }
            CaptureMode::Region => {
                command.arg("-i");
            }
            CaptureMode::Window => {
                command.args(["-i", "-w"]);
            }
        }
        command.arg(&path);
        match command.output() {
            Ok(_) if is_png(&path) => CaptureOutcome::Success(CaptureArtifact {
                path,
                _directory: directory,
            }),
            Ok(output) if output.status.success() || output.stderr.is_empty() => {
                CaptureOutcome::Cancelled
            }
            Ok(output) => {
                let message = String::from_utf8_lossy(&output.stderr);
                // Escape is reported as an unsuccessful interactive capture on some OS versions.
                if message.to_lowercase().contains("cancel")
                    || message.trim() == "screencapture: no image captured"
                {
                    CaptureOutcome::Cancelled
                } else {
                    CaptureOutcome::Error(format!(
                        "O macOS não conseguiu capturar a tela. Verifique a permissão em Ajustes do Sistema → Privacidade e Segurança → Gravação de Tela.\n\n{}",
                        message.trim()
                    ))
                }
            }
            Err(e) => {
                CaptureOutcome::Error(format!("Não foi possível iniciar a captura do macOS: {e}"))
            }
        }
    }
}

fn is_png(path: &Path) -> bool {
    use std::io::Read;
    let mut signature = [0_u8; 8];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut signature))
        .is_ok()
        && signature == *b"\x89PNG\r\n\x1a\n"
}
