use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug)]
pub enum CaptureMode {
    Screen,
    Region,
    Window,
}

/// Owns the temporary capture. Dropping it removes the original file.
pub struct CaptureArtifact {
    pub path: PathBuf,
    pub _directory: TempDir,
}

pub enum CaptureOutcome {
    Success(CaptureArtifact),
    Cancelled,
    Error(String),
}

/// Platform boundary: no window-system types escape the adapter.
pub trait CaptureBackend {
    fn capture(&self, mode: CaptureMode) -> CaptureOutcome;
}
