//! Opt-in tests requiring a logged-in macOS desktop. No screen capture is performed.
#![cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/macos/shortcuts.rs"]
mod shortcuts;

use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires a logged-in macOS desktop; opens synthetic editor windows"]
fn executable_completes_50_capture_copy_export_cleanup_cycles() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("results");
    let temporary = root.path().join("tmp");
    fs::create_dir(&temporary).unwrap();
    let log_path = root.path().join("process.log");
    let log = fs::File::create(&log_path).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_rxs"))
        .arg("--diagnostics")
        // Isolate session cleanup and locking from any real RXS instance.
        .env("TMPDIR", &temporary)
        .env("RXS_DIAGNOSTICS_DIR", &output)
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "native diagnostics timed out: {}",
            fs::read_to_string(&log_path).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(status.success(), "diagnostics failed ({status}): {log}");
    assert!(
        log.contains("Native checks passed."),
        "incomplete run: {log}"
    );
    let report = fs::read_to_string(output.join("native-checks.txt")).unwrap();
    for expected in [
        "PASS:",
        "private clipboard PNG roundtrip",
        "zero retained canvases",
        "zero temporary captures",
    ] {
        assert!(
            report.contains(expected),
            "missing assertion report: {expected}"
        );
    }
    let csv = fs::read_to_string(output.join("memory.csv")).unwrap();
    for phase in ["editing", "export", "closed"] {
        for index in 1..=50 {
            assert!(
                csv.lines()
                    .any(|line| line.starts_with(&format!("{phase}_{index},"))),
                "missing cycle: {phase}_{index}"
            );
        }
    }
    assert!(csv.lines().any(|line| line.starts_with("idle_settled,")));
    for (file, dimensions) in [
        ("source-4k.png", (3840, 2160)),
        ("annotated-4k.png", (3840, 2160)),
        ("shortcuts-light.png", (640, 440)),
        ("shortcuts-dark.png", (640, 440)),
    ] {
        let bytes = fs::read(output.join(file)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&bytes[12..16], b"IHDR");
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!((width, height), dimensions, "wrong dimensions: {file}");
    }
    // A complete diagnostic must have used our isolated capture root.
    let sessions: Vec<_> = fs::read_dir(&temporary)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("app.rxs.capture-")
        })
        .collect();
    assert_eq!(sessions.len(), 1);
    assert!(fs::read_dir(&sessions[0]).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("session-")
    }));
}

// A second GlobalHotKeyManager cannot install the same Carbon event handler in
// this process. Reserve competing combinations directly, without an event handler.
#[repr(C)]
struct TestHotKeyId {
    signature: u32,
    id: u32,
}
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut std::ffi::c_void;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: TestHotKeyId,
        target: *mut std::ffi::c_void,
        options: u32,
        reference: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut std::ffi::c_void) -> i32;
}
#[derive(Default)]
struct CompetingRegistrations(std::collections::HashMap<u32, *mut std::ffi::c_void>);
impl CompetingRegistrations {
    fn register(&mut self, key: global_hotkey::hotkey::HotKey) -> Result<(), i32> {
        assert!(
            !self.0.contains_key(&key.id()),
            "test registered a duplicate it already owns"
        );
        let code = (0..128)
            .find(|code| shortcuts::from_keycode(*code, key.mods).ok() == Some(key))
            .unwrap();
        let mut reference = std::ptr::null_mut();
        // All test combinations use Command + Control + Option + Shift.
        let status = unsafe {
            RegisterEventHotKey(
                code.into(),
                256 | 512 | 2048 | 4096,
                TestHotKeyId {
                    signature: u32::from_be_bytes(*b"rxst"),
                    id: key.id(),
                },
                GetApplicationEventTarget(),
                0,
                &mut reference,
            )
        };
        if status != 0 {
            return Err(status);
        }
        self.0.insert(key.id(), reference);
        Ok(())
    }
    fn unregister(&mut self, key: global_hotkey::hotkey::HotKey) -> Result<(), i32> {
        let reference = *self.0.get(&key.id()).expect("test registration not owned");
        let status = unsafe { UnregisterEventHotKey(reference) };
        if status != 0 {
            return Err(status);
        }
        self.0.remove(&key.id());
        Ok(())
    }
}
impl Drop for CompetingRegistrations {
    fn drop(&mut self) {
        for reference in self.0.values() {
            unsafe {
                UnregisterEventHotKey(*reference);
            }
        }
    }
}

#[test]
#[ignore = "requires macOS native hotkey service; temporarily registers available test combinations"]
fn global_shortcuts_suspend_resume_and_rollback_after_registration_conflict() {
    let mut keys = shortcuts::Shortcuts::new().unwrap();
    let mut holder = CompetingRegistrations::default();
    let mut available = Vec::new();
    // Reserve only combinations confirmed to be free. Drop releases every registration.
    for letter in 'A'..='Z' {
        let key = shortcuts::parse(&format!("Command+Control+Option+Shift+{letter}")).unwrap();
        if !shortcuts::system_conflict(key).unwrap() && holder.register(key).is_ok() {
            available.push(key);
            if available.len() == 4 {
                break;
            }
        }
    }
    assert_eq!(
        available.len(),
        4,
        "need four free combinations to exercise the real registration service"
    );
    for key in &available[..3] {
        holder.unregister(*key).unwrap();
    }
    let text: Vec<_> = available
        .iter()
        .copied()
        .map(shortcuts::canonical)
        .collect();
    keys.configure(&text[0], &text[1]).unwrap();
    let original = keys.active.clone();
    assert_eq!(original, available[..2]);

    // Invalid drafts must not replace active registrations.
    assert!(keys.configure(&text[0], &text[0]).is_err());
    assert!(keys.configure("Command+C", &text[1]).is_err());
    assert_eq!(keys.active, original);

    keys.suspend().unwrap();
    assert!(keys.active.is_empty());
    keys.suspend().unwrap(); // repeated cancellation preparation is harmless
    for key in &original {
        holder.register(*key).unwrap();
        holder.unregister(*key).unwrap();
    }
    keys.resume().unwrap();
    keys.resume().unwrap();
    assert_eq!(keys.active, original);
    for key in &original {
        assert!(holder.register(*key).is_err());
    }

    // The first new registration succeeds; the second is held by the competing manager.
    let error = keys.configure(&text[2], &text[3]).unwrap_err();
    assert!(error.contains("anteriores foram preservados"), "{error}");
    assert_eq!(keys.active, original);
    holder.register(available[2]).unwrap(); // partial proposal must have been cleaned up
    holder.unregister(available[2]).unwrap();
    for key in &original {
        assert!(holder.register(*key).is_err());
    }

    // Resume failure must leave no partial pair that could dispatch the wrong mode.
    keys.suspend().unwrap();
    holder.register(original[1]).unwrap();
    assert!(keys.resume().is_err());
    assert!(keys.active.is_empty());
    holder.register(original[0]).unwrap();
    holder.unregister(original[0]).unwrap();
    holder.unregister(original[1]).unwrap();
    holder.unregister(available[3]).unwrap();
    keys.configure(&text[2], &text[3]).unwrap();
    assert_eq!(keys.active, available[2..]);
    drop(keys);
    for key in available {
        holder.register(key).unwrap();
        holder.unregister(key).unwrap();
    }
}
