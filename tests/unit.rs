//! Import the real modules because RXS currently has only a binary target.
//! No production visibility or behavior changes are needed for these tests.
#[allow(dead_code)]
#[path = "../src/document.rs"]
mod document;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/macos/shortcuts.rs"]
mod shortcuts;

use document::{Annotation, Document, Point, Style, Tool};

fn stroke(tool: Tool) -> Annotation {
    let mut stroke = Annotation::new(tool, Style::default(), Point::new(10.0, 20.0));
    stroke.update(Point::new(90.0, 70.0));
    stroke
}

#[test]
fn export_then_edit_undo_and_redo_tracks_the_exact_exported_version() {
    let mut doc = Document::new(100.0, 100.0);
    assert!(doc.needs_export());
    doc.mark_exported();
    for tool in [Tool::Arrow, Tool::Rectangle, Tool::Freehand] {
        doc.push(stroke(tool));
        assert!(doc.needs_export());
        doc.undo();
        assert!(!doc.needs_export());
        doc.redo();
        assert!(doc.needs_export());
        doc.mark_exported();
        assert!(!doc.needs_export());
    }
}

#[test]
fn invisible_stroke_preserves_export_and_redo_history() {
    let mut doc = Document::new(100.0, 100.0);
    doc.mark_exported();
    doc.push(stroke(Tool::Arrow));
    doc.undo();
    let mut jitter = Annotation::new(Tool::Freehand, Style::default(), Point::default());
    jitter.update(Point::new(0.3, 0.3));
    doc.push(jitter);
    assert!(doc.can_redo());
    assert!(!doc.needs_export());
    assert_eq!(doc.annotations().count(), 0);
}

#[test]
fn tool_and_style_choices_do_not_dirty_an_exported_image() {
    let mut doc = Document::new(100.0, 100.0);
    doc.mark_exported();
    doc.tool = Tool::Rectangle;
    doc.style.width = 12.0;
    doc.style.color = [0.1, 0.2, 0.3, 0.4];
    assert!(!doc.needs_export());
}

#[test]
fn exporting_is_idempotent_and_does_not_destroy_undo() {
    let mut doc = Document::new(100.0, 100.0);
    doc.push(stroke(Tool::Rectangle));
    doc.mark_exported();
    doc.mark_exported();
    assert!(doc.can_undo());
    doc.undo();
    assert!(doc.needs_export());
    doc.redo();
    assert!(!doc.needs_export());
}

#[test]
fn drawing_after_undo_never_reuses_the_exported_revision() {
    let mut doc = Document::new(100.0, 100.0);
    doc.push(stroke(Tool::Arrow));
    doc.mark_exported();
    doc.undo();
    doc.push(stroke(Tool::Rectangle));
    assert!(doc.needs_export());
    assert!(!doc.can_redo());
    assert_eq!(doc.annotations().next().unwrap().tool, Tool::Rectangle);
}

#[test]
fn freehand_filters_jitter_and_bounds_stroke_size() {
    let mut line = Annotation::new(Tool::Freehand, Style::default(), Point::default());
    line.update(Point::new(0.5, 0.0));
    assert_eq!(line.points.len(), 1);
    line.update(Point::new(0.75, 0.0));
    assert_eq!(line.points.len(), 2);
    for x in 1..100_100 {
        line.update(Point::new(x as f64, 0.0));
    }
    assert_eq!(line.points.len(), 100_000);
}

#[cfg(target_os = "macos")]
mod shortcut_rules {
    use super::shortcuts::*;
    use global_hotkey::hotkey::{Code, Modifiers};

    #[test]
    fn all_letters_and_digits_are_accepted_with_each_supported_modifier_set() {
        for extra in [
            Modifiers::ALT,
            Modifiers::SHIFT,
            Modifiers::CONTROL,
            Modifiers::ALT | Modifiers::SHIFT | Modifiers::CONTROL,
        ] {
            for key in ('A'..='Z').chain('0'..='9') {
                let prefix = if extra == Modifiers::ALT {
                    "Command+Option"
                } else if extra == Modifiers::SHIFT {
                    "Command+Shift"
                } else if extra == Modifiers::CONTROL {
                    "Command+Control"
                } else {
                    "Command+Control+Option+Shift"
                };
                let input = format!("{prefix}+{key}");
                if input == "Command+Shift+5" {
                    assert!(parse(&input).is_err());
                } else {
                    let hotkey = parse(&input).unwrap();
                    assert_eq!(hotkey.mods, Modifiers::SUPER | extra);
                }
            }
        }
    }

    #[test]
    fn unmodified_keys_and_editor_combinations_cannot_be_global_capture_shortcuts() {
        for input in [
            "",
            "A",
            "Shift+A",
            "Option+3",
            "Control+Shift+3",
            "Command+C",
            "Command+S",
            "Command+Option+F1",
            "Command+Shift+Escape",
            "Command+Shift+5",
            "not a shortcut",
        ] {
            assert!(parse(input).is_err(), "unexpectedly accepted {input}");
        }
    }

    #[test]
    fn same_combination_with_different_spelling_is_still_a_duplicate() {
        assert!(validate_pair("Command+Option+a", "Option+Command+KeyA").is_err());
        let pair = validate_pair("Command+Option+3", "Command+Option+4").unwrap();
        assert_eq!(pair[0].key, Code::Digit3);
        assert_eq!(pair[1].key, Code::Digit4);
    }

    #[test]
    fn native_mapping_has_exactly_36_keys_and_distinct_identities() {
        let keys: Vec<_> = (0..=255)
            .filter_map(|code| from_keycode(code, Modifiers::SUPER | Modifiers::ALT).ok())
            .collect();
        assert_eq!(keys.len(), 36);
        let ids: std::collections::HashSet<_> = keys.iter().map(|key| key.id()).collect();
        assert_eq!(ids.len(), 36);
        // Independent hardware examples catch accidental permutations in the mapping.
        for (code, expected) in [
            (0, Code::KeyA),
            (8, Code::KeyC),
            (1, Code::KeyS),
            (20, Code::Digit3),
            (21, Code::Digit4),
            (29, Code::Digit0),
        ] {
            assert_eq!(
                from_keycode(code, Modifiers::SUPER | Modifiers::ALT)
                    .unwrap()
                    .key,
                expected
            );
        }
    }

    #[test]
    fn legacy_values_roundtrip_to_persistable_strings_and_readable_symbols() {
        for value in [
            "Command+Shift+l",
            "Option+Command+KeyA",
            "Command+Control+Option+Shift+9",
        ] {
            let hotkey = parse(value).unwrap();
            assert_eq!(parse(&canonical(hotkey)).unwrap(), hotkey);
        }
        assert_eq!(symbols("Command+Shift+l"), "⌘ ⇧ L");
        assert_eq!(symbols("Command+Control+Option+Shift+9"), "⌘ ⌃ ⌥ ⇧ 9");
    }
}
