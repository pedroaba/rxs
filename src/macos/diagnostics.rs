//! Opt-in local harness. It never captures the user's screen or changes shortcuts.
use super::*;
use crate::{
    capture::CaptureArtifact,
    document::{Annotation, Point, Style},
};
use objc2::{AnyThread, rc::autoreleasepool};
use objc2_core_graphics::CGContext;
use std::{fs, path::PathBuf};

#[derive(Clone, Copy)]
struct Memory {
    footprint: u64,
    resident: u64,
}
fn memory() -> Memory {
    let mut usage: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            (&mut usage as *mut libc::rusage_info_v2).cast(),
        )
    };
    assert_eq!(status, 0, "memory measurement failed");
    Memory {
        footprint: usage.ri_phys_footprint,
        resident: usage.ri_resident_size,
    }
}
fn record(rows: &mut Vec<String>, phase: &str) {
    let usage = memory();
    rows.push(format!(
        "{phase},{:.2},{:.2}",
        usage.footprint as f64 / 1_000_000.0,
        usage.resident as f64 / 1_000_000.0
    ));
}

fn fixture(app: &App) -> Result<CaptureArtifact, String> {
    let directory = tempfile::Builder::new()
        .prefix("session-")
        .tempdir_in(&app.ivars().session.root)
        .map_err(|e| e.to_string())?;
    let path = directory.path().join("capture.png");
    let bitmap = render::new_bitmap(3840, 2160)?;
    let stride = bitmap.bytesPerRow() as usize;
    // The bitmap owns a contiguous, writable RGBA plane for this entire scope.
    let data = unsafe { std::slice::from_raw_parts_mut(bitmap.bitmapData(), stride * 2160) };
    for y in 0..2160 {
        for x in 0..3840 {
            let index = y * stride + x * 4;
            let color = if x < 120 && y < 120 {
                [255, 0, 0, 255]
            } else if x < 120 && y >= 2040 {
                [0, 0, 255, 255]
            } else if x >= 3720 && y < 120 {
                [0, 0, 0, 0]
            } else if x > 240 && x < 3600 && y > 360 && y < 1850 {
                [255, 255, 255, 255]
            } else {
                [236 + (x % 9) as u8, 239 + (y % 9) as u8, 248, 255]
            };
            data[index..index + 4].copy_from_slice(&color);
        }
    }
    let context =
        NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap).ok_or("fixture context")?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let cg = context.CGContext();
    // Native, synthetic content resembles a screenshot while remaining private.
    for (x, width, color) in [
        (400.0, 850.0, [0.18, 0.36, 0.88]),
        (1400.0, 680.0, [0.12, 0.60, 0.47]),
        (2230.0, 1000.0, [0.9, 0.54, 0.2]),
    ] {
        CGContext::set_rgb_fill_color(Some(&cg), color[0], color[1], color[2], 1.0);
        CGContext::fill_rect(Some(&cg), render::rect(x, 1100.0, width, 240.0));
        for line in 0..5 {
            CGContext::set_rgb_fill_color(Some(&cg), 0.83, 0.86, 0.91, 1.0);
            CGContext::fill_rect(
                Some(&cg),
                render::rect(
                    x,
                    950.0 - line as f64 * 75.0,
                    width * (1.0 - line as f64 * 0.08),
                    22.0,
                ),
            );
        }
    }
    NSGraphicsContext::restoreGraphicsState_class();
    let png = render::png(&bitmap)?;
    fs::write(&path, unsafe { png.as_bytes_unchecked() }).map_err(|e| e.to_string())?;
    Ok(CaptureArtifact {
        path,
        _directory: directory,
    })
}

fn annotate(editor: &Editor) {
    let mut doc = editor.canvas.ivars().document.borrow_mut();
    let style = Style {
        color: [0.95, 0.18, 0.16, 1.0],
        width: 12.0,
    };
    let mut arrow = Annotation::new(Tool::Arrow, style, Point::new(500.0, 450.0));
    arrow.update(Point::new(1000.0, 900.0));
    doc.push(arrow);
    let mut rectangle = Annotation::new(Tool::Rectangle, style, Point::new(1360.0, 760.0));
    rectangle.update(Point::new(2120.0, 1120.0));
    doc.push(rectangle);
    let mut freehand = Annotation::new(Tool::Freehand, style, Point::new(2250.0, 1500.0));
    for index in 1..250 {
        freehand.update(Point::new(
            2250.0 + index as f64 * 4.0,
            1500.0 + (index as f64 / 20.0).sin() * 55.0,
        ));
    }
    doc.push(freehand);
    let mut translucent = Annotation::new(
        Tool::Rectangle,
        Style {
            color: [1.0, 0.0, 0.0, 0.5],
            width: 8.0,
        },
        Point::new(3740.0, 30.0),
    );
    translucent.update(Point::new(3810.0, 90.0));
    doc.push(translucent);
    drop(doc);
    editor.canvas.setNeedsDisplay(true);
    editor.refresh();
}

fn verify(editor: &Editor, output: &std::path::Path) -> Result<(), String> {
    let many = editor
        .color_picker_many_snapshot()
        .ok_or("palette overflow snapshot")?;
    let many_png = render::png(&many)?;
    fs::write(output.join("color-picker-many.png"), unsafe {
        many_png.as_bytes_unchecked()
    })
    .map_err(|e| e.to_string())?;
    let picker = editor
        .color_picker_snapshot()
        .ok_or("color picker snapshot")?;
    let picker_png = render::png(&picker)?;
    fs::write(output.join("color-picker.png"), unsafe {
        picker_png.as_bytes_unchecked()
    })
    .map_err(|e| e.to_string())?;
    let bitmap = render::render_bitmap(
        &editor.canvas.ivars().image,
        &editor.canvas.ivars().document.borrow(),
    )?;
    let rgba = |x, y| -> [f64; 4] {
        let color = bitmap
            .colorAtX_y(x, y)
            .unwrap()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .unwrap();
        [
            color.redComponent(),
            color.greenComponent(),
            color.blueComponent(),
            color.alphaComponent(),
        ]
    };
    let top = rgba(20, 20);
    let bottom = rgba(20, 2140);
    let transparent = rgba(3800, 20);
    let arrow = rgba(750, 675);
    let translucent = rgba(3770, 30);
    assert!(
        (translucent[3] - 0.5).abs() < 0.02,
        "annotation alpha lost: {translucent:?}"
    );
    assert!(
        top[0] > 0.95 && top[2] < 0.05,
        "top-left marker inverted: {top:?}"
    );
    assert!(
        bottom[2] > 0.95 && bottom[0] < 0.05,
        "bottom-left marker inverted: {bottom:?}"
    );
    assert!(
        transparent[3] < 0.05,
        "alpha not preserved: {transparent:?}"
    );
    assert!(
        arrow[0] > 0.8 && arrow[1] < 0.4,
        "arrow not in pixel coordinates: {arrow:?}"
    );
    assert_eq!((bitmap.pixelsWide(), bitmap.pixelsHigh()), (3840, 2160));
    let png = render::png(&bitmap)?;
    fs::write(output.join("annotated-4k.png"), unsafe {
        png.as_bytes_unchecked()
    })
    .map_err(|e| e.to_string())?;
    // Test clipboard serialization using a private pasteboard, leaving the
    // user's general clipboard intact. The editor uses these same PNG bytes.
    let pasteboard = NSPasteboard::pasteboardWithUniqueName();
    assert!(pasteboard.setData_forType(Some(&png), unsafe { NSPasteboardTypePNG }));
    let readback = pasteboard
        .dataForType(unsafe { NSPasteboardTypePNG })
        .ok_or("clipboard readback failed")?;
    let decoded = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &readback)
        .ok_or("clipboard PNG invalid")?;
    assert_eq!((decoded.pixelsWide(), decoded.pixelsHigh()), (3840, 2160));
    assert!(
        (decoded.colorAtX_y(3770, 30).unwrap().alphaComponent() - 0.5).abs() < 0.02,
        "PNG lost annotation alpha"
    );
    // AppKit's named-pasteboard cleanup is not exposed by objc2-app-kit.
    unsafe {
        let _: () = msg_send![&*pasteboard, releaseGlobally];
    }
    Ok(())
}

pub fn run(app: &App, demo: bool) {
    let mut rows = vec!["phase,physical_footprint_mb,resident_mb".into()];
    record(&mut rows, "idle_start");
    let output = std::env::var_os("RXS_DIAGNOSTICS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/diagnostics"));
    fs::create_dir_all(&output).expect("create diagnostics output");
    let fixture = autoreleasepool(|_| fixture(app)).expect("create 4K fixture");
    let fixture_path = output.join("source-4k.png");
    fs::copy(&fixture.path, &fixture_path).expect("save fixture");
    drop(fixture);
    if demo {
        let artifact = copy_fixture(app, &fixture_path);
        app.finish_capture(CaptureOutcome::Success(artifact));
        annotate(&app.editor().unwrap());
        println!("RXS demo opened; synthetic 3840×2160 image.");
    } else {
        cycle(0, rows, output, fixture_path);
    }
}

fn copy_fixture(app: &App, source: &std::path::Path) -> CaptureArtifact {
    let directory = tempfile::Builder::new()
        .prefix("session-")
        .tempdir_in(&app.ivars().session.root)
        .unwrap();
    let path = directory.path().join("capture.png");
    fs::copy(source, &path).unwrap();
    CaptureArtifact {
        path,
        _directory: directory,
    }
}

fn later(milliseconds: u64, work: impl FnOnce() + Send + 'static) {
    let when =
        dispatch2::DispatchTime::try_from(std::time::Duration::from_millis(milliseconds)).unwrap();
    DispatchQueue::main().after(when, work).unwrap();
}

fn cycle(index: usize, mut rows: Vec<String>, output: PathBuf, source: PathBuf) {
    autoreleasepool(|_| {
        with_app(|app| {
            let artifact = copy_fixture(app, &source);
            app.finish_capture(CaptureOutcome::Success(artifact));
            let editor = app.editor().unwrap();
            annotate(&editor);
            editor.window.displayIfNeeded();
        })
    });
    // Give AppKit/WindowServer an actual presentation turn before measuring.
    later(250, move || {
        autoreleasepool(|_| {
            with_app(|app| {
                let editor = app.editor().unwrap();
                record(&mut rows, &format!("editing_{}", index + 1));
                if index == 0 {
                    editor.verify_editor_layout();
                    verify(&editor, &output).expect("native pixel / clipboard assertions");
                }
                let exported = editor.canvas.export().expect("export 4K PNG");
                record(&mut rows, &format!("export_{}", index + 1));
                drop(exported);
                editor.canvas.mark_exported();
                app.close_editor();
                assert_eq!(
                    NSApplication::sharedApplication(app.mtm()).activationPolicy(),
                    NSApplicationActivationPolicy::Accessory
                );
            })
        });
        later(250, move || {
            assert_eq!(
                editor::LIVE_CANVASES.load(std::sync::atomic::Ordering::Relaxed),
                0,
                "closed canvas retained"
            );
            with_app(|app| {
                let remaining = fs::read_dir(&app.ivars().session.root)
                    .unwrap()
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("session-"))
                    .count();
                assert_eq!(remaining, 0, "temporary capture leaked");
            });
            record(&mut rows, &format!("closed_{}", index + 1));
            if index < 49 {
                cycle(index + 1, rows, output, source);
            } else {
                later(2000, move || {
                    record(&mut rows, "idle_settled");
                    fs::write(output.join("memory.csv"), rows.join("\n") + "\n").unwrap();
                    fs::write(output.join("native-checks.txt"), "PASS: centered image at fit/10%/25%/100% and resized viewport, floating color picker open/close without viewport changes, RGBA propagation, editor activation policy, 3840×2160 export, top/bottom orientation, transparency, annotation pixel coordinates, private clipboard PNG roundtrip, 50 editor/export/close cycles, zero retained canvases and zero temporary captures after each close.\n").unwrap();
                    if let Ok(map) = std::process::Command::new("/usr/bin/vmmap")
                        .args(["-summary", &std::process::id().to_string()])
                        .output()
                    {
                        let _ = fs::write(output.join("vmmap-after-50.txt"), &map.stdout);
                    }
                    println!(
                        "Native checks passed. Measurements: {}",
                        output.join("memory.csv").display()
                    );
                    with_app(|app| NSApplication::sharedApplication(app.mtm()).terminate(None));
                });
            }
        });
    });
}

pub fn write_icon(path: &std::path::Path) -> Result<(), String> {
    let bitmap = render::new_bitmap(1024, 1024)?;
    let context =
        NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap).ok_or("icon context")?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    NSColor::colorWithSRGBRed_green_blue_alpha(0.06, 0.18, 0.23, 1.0).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        render::rect(64.0, 64.0, 896.0, 896.0),
        200.0,
        200.0,
    )
    .fill();
    NSColor::whiteColor().setStroke();
    let corners = NSBezierPath::bezierPath();
    corners.setLineWidth(38.0);
    corners.setLineCapStyle(NSLineCapStyle::Round);
    corners.setLineJoinStyle(NSLineJoinStyle::Round);
    for (x, y, dx, dy) in [
        (270.0, 280.0, 1.0, 1.0),
        (754.0, 280.0, -1.0, 1.0),
        (270.0, 744.0, 1.0, -1.0),
        (754.0, 744.0, -1.0, -1.0),
    ] {
        corners.moveToPoint(objc2_foundation::NSPoint::new(x, y + dy * 115.0));
        corners.lineToPoint(objc2_foundation::NSPoint::new(x, y));
        corners.lineToPoint(objc2_foundation::NSPoint::new(x + dx * 115.0, y));
    }
    corners.stroke();
    NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.39, 0.29, 1.0).setStroke();
    let arrow = NSBezierPath::bezierPath();
    arrow.setLineWidth(42.0);
    arrow.setLineCapStyle(NSLineCapStyle::Round);
    arrow.setLineJoinStyle(NSLineJoinStyle::Round);
    for (index, (x, y)) in [
        (375.0, 370.0),
        (635.0, 650.0),
        (480.0, 650.0),
        (635.0, 650.0),
        (635.0, 490.0),
    ]
    .iter()
    .enumerate()
    {
        if index == 0 {
            arrow.moveToPoint(objc2_foundation::NSPoint::new(*x, *y));
        } else {
            arrow.lineToPoint(objc2_foundation::NSPoint::new(*x, *y));
        }
    }
    arrow.stroke();
    NSGraphicsContext::restoreGraphicsState_class();
    let png = render::png(&bitmap)?;
    fs::write(path, unsafe { png.as_bytes_unchecked() }).map_err(|e| e.to_string())
}
