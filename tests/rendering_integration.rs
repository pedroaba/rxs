#![cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/capture.rs"]
mod capture;
#[allow(dead_code)]
#[path = "../src/document.rs"]
mod document;
#[allow(dead_code)]
#[path = "../src/macos/render.rs"]
mod render;

use document::{Annotation, Document, Point, Style, Tool};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSColorSpace;
use std::fs;

fn fixture() -> capture::CaptureArtifact {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture.png");
    let bitmap = render::new_bitmap(64, 48).unwrap();
    let stride = bitmap.bytesPerRow() as usize;
    let bytes = unsafe { std::slice::from_raw_parts_mut(bitmap.bitmapData(), stride * 48) };
    bytes.fill(0);
    for y in 0..48 {
        for x in 0..64 {
            let color = if x < 8 && y < 8 {
                [255, 0, 0, 255]
            } else if x < 8 && y >= 40 {
                [0, 0, 255, 255]
            } else {
                [0, 0, 0, 0]
            };
            bytes[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&color);
        }
    }
    let data = render::png(&bitmap).unwrap();
    fs::write(&path, unsafe { data.as_bytes_unchecked() }).unwrap();
    capture::CaptureArtifact {
        path,
        _directory: directory,
    }
}

fn pixel(image: &render::NativeImage, x: isize, y: isize) -> [f64; 4] {
    let c = image
        .bitmap
        .colorAtX_y(x, y)
        .unwrap()
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .unwrap();
    [
        c.redComponent(),
        c.greenComponent(),
        c.blueComponent(),
        c.alphaComponent(),
    ]
}

#[test]
fn png_file_roundtrip_preserves_dimensions_orientation_and_alpha() {
    autoreleasepool(|_| {
        let artifact = fixture();
        let source = render::NativeImage::load(&artifact.path).unwrap();
        let doc = Document::new(source.width, source.height);
        let exported = render::export(&source, &doc).unwrap();
        let destination = artifact.path.with_file_name("export.png");
        fs::write(&destination, unsafe { exported.as_bytes_unchecked() }).unwrap();
        let decoded = render::NativeImage::load(&destination).unwrap();
        assert_eq!((decoded.width, decoded.height), (64.0, 48.0));
        assert!(pixel(&decoded, 2, 2)[0] > 0.95);
        assert!(pixel(&decoded, 2, 45)[2] > 0.95);
        assert!(pixel(&decoded, 60, 20)[3] < 0.01);
        // Rendering alone is not proof of a successful copy/save.
        assert!(doc.needs_export());
    });
}

#[test]
fn annotations_change_exported_pixels_and_undo_restores_the_source() {
    autoreleasepool(|_| {
        let artifact = fixture();
        let source = render::NativeImage::load(&artifact.path).unwrap();
        let mut doc = Document::new(source.width, source.height);
        doc.mark_exported();
        let mut line = Annotation::new(
            Tool::Freehand,
            Style {
                color: [0.0, 1.0, 0.0, 1.0],
                width: 6.0,
            },
            Point::new(20.0, 24.0),
        );
        line.update(Point::new(50.0, 24.0));
        doc.push(line);
        let bitmap = render::render_bitmap(&source, &doc).unwrap();
        let color = bitmap
            .colorAtX_y(35, 24)
            .unwrap()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .unwrap();
        assert!(color.greenComponent() > 0.95 && color.alphaComponent() > 0.95);
        assert!(doc.needs_export());
        doc.undo();
        let restored = render::render_bitmap(&source, &doc).unwrap();
        assert!(restored.colorAtX_y(35, 24).unwrap().alphaComponent() < 0.01);
        assert!(!doc.needs_export());
    });
}

#[test]
fn missing_and_corrupt_images_return_errors() {
    autoreleasepool(|_| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.png");
        assert!(render::NativeImage::load(&path).is_err());
        for bytes in [b"".as_slice(), b"not an image", b"\x89PNG\r\n\x1a\n"] {
            fs::write(&path, bytes).unwrap();
            assert!(render::NativeImage::load(&path).is_err());
        }
    });
}

#[test]
fn dropping_capture_removes_its_files_but_preserves_saved_export() {
    autoreleasepool(|_| {
        let artifact = fixture();
        let original_path = artifact.path.clone();
        let original_dir = original_path.parent().unwrap().to_path_buf();
        let destination = tempfile::tempdir().unwrap();
        let saved = destination.path().join("saved.png");
        let source = render::NativeImage::load(&artifact.path).unwrap();
        let png = render::export(&source, &Document::new(64.0, 48.0)).unwrap();
        fs::write(&saved, unsafe { png.as_bytes_unchecked() }).unwrap();
        drop(source);
        drop(artifact);
        assert!(!original_dir.exists());
        assert!(!original_path.exists());
        assert!(render::NativeImage::load(&saved).is_ok());
    });
}
