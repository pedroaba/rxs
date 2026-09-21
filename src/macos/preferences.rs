//! Native shortcut preferences. Draft values are committed only after registration succeeds.
use super::{App, editor::label, render::rect, shortcuts};
use global_hotkey::hotkey::Modifiers;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{NSNotification, NSObjectProtocol, NSString, NSUserDefaults};
use std::cell::{Cell, RefCell};

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct PreferencesContent;
    unsafe impl NSObjectProtocol for PreferencesContent {}
    impl PreferencesContent {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: objc2_foundation::NSRect) {
            NSColor::windowBackgroundColor().setFill();
            NSBezierPath::bezierPathWithRect(self.bounds()).fill();
            NSColor::controlBackgroundColor().setFill();
            for y in [213.0, 101.0] {
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(16.0,y,608.0,104.0), 10.0, 10.0).fill();
            }
        }
    }
);

#[derive(Default)]
pub struct PreferencesIvars {
    values: RefCell<[String; 2]>,
    recording: Cell<Option<usize>>,
    buttons: RefCell<Vec<Retained<NSButton>>>,
    errors: RefCell<Vec<Retained<NSTextField>>>,
}

define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PreferencesIvars]
    struct Preferences;
    unsafe impl NSObjectProtocol for Preferences {}
    unsafe impl NSWindowDelegate for Preferences {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _window: &NSWindow) -> bool { self.cancel(sel!(cancel:), None); false }
        #[unsafe(method(windowDidResignKey:))]
        fn resigned(&self, _notification: &NSNotification) { self.end_recording(); }
    }
    impl Preferences {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            if let Some(index) = self.ivars().recording.get() {
                match event.r#type() {
                    NSEventType::FlagsChanged => {
                        let preview = modifier_symbols(modifiers(event.modifierFlags()));
                        let title = if preview.is_empty() { "Pressione a combinação…".to_string() } else { format!("{preview} …") };
                        self.ivars().buttons.borrow()[index].setTitle(&NSString::from_str(&title));
                        return;
                    }
                    NSEventType::KeyDown => {
                        if event.isARepeat() { return; }
                        if event.keyCode() == 53 { self.end_recording(); return; }
                        // Tab cancels recording and continues normal keyboard navigation.
                        if event.keyCode() == 48 { self.end_recording(); }
                        else { self.record(index, event); return; }
                    }
                    NSEventType::LeftMouseDown | NSEventType::RightMouseDown => self.end_recording(),
                    _ => {}
                }
            }
            unsafe { let _: () = msg_send![super(self), sendEvent: event]; }
        }
        #[unsafe(method(recordShortcut:))]
        fn start_recording(&self, sender: &NSButton) {
            self.end_recording();
            let index = sender.tag() as usize;
            let mut result = Ok(());
            super::with_app(|app| {
                if let Some(keys) = app.ivars().shortcuts.borrow_mut().as_mut() { result = keys.suspend(); }
            });
            if let Err(error) = result { self.error(index, &error); return; }
            self.error(index, "");
            self.ivars().recording.set(Some(index));
            sender.setTitle(&NSString::from_str("Pressione a combinação…"));
        }
        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            self.end_recording();
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
        #[unsafe(method(apply:))]
        fn apply(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            self.end_recording();
            let values = self.ivars().values.borrow();
            if let Err(error) = shortcuts::validate_pair(&values[0], &values[1]) { self.error(1, &error); return; }
            for (i, value) in values.iter().enumerate() {
                let result = shortcuts::parse(value).and_then(shortcuts::system_conflict);
                match result {
                    Ok(false) => self.error(i, ""),
                    Ok(true) => { self.error(i, "Combinação ativa no macOS. Escolha outra ou abra Ajustes de Teclado."); return; }
                    Err(error) => { self.error(i, &error); return; }
                }
            }
            let mut result = Ok(());
            super::with_app(|app| result = app.configure_shortcuts(&values[0], &values[1]));
            if let Err(error) = result { self.error(1, &error); return; }
            let defaults = NSUserDefaults::standardUserDefaults();
            for (key, value) in ["screenShortcut", "regionShortcut"].into_iter().zip(values.iter()) {
                unsafe { defaults.setObject_forKey(Some(&NSString::from_str(value)), &NSString::from_str(key)); }
            }
            defaults.setBool_forKey(true, &NSString::from_str("shortcutsEnabled"));
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            self.end_recording();
            super::open_settings("x-apple.systempreferences:com.apple.Keyboard-Settings.extension");
        }
    }
);

fn modifiers(flags: NSEventModifierFlags) -> Modifiers {
    let mut result = Modifiers::empty();
    for (native, flag) in [
        (NSEventModifierFlags::Command, Modifiers::SUPER),
        (NSEventModifierFlags::Control, Modifiers::CONTROL),
        (NSEventModifierFlags::Option, Modifiers::ALT),
        (NSEventModifierFlags::Shift, Modifiers::SHIFT),
    ] {
        if flags.contains(native) {
            result |= flag;
        }
    }
    result
}
fn modifier_symbols(mods: Modifiers) -> String {
    [
        (Modifiers::CONTROL, "⌃"),
        (Modifiers::ALT, "⌥"),
        (Modifiers::SHIFT, "⇧"),
        (Modifiers::SUPER, "⌘"),
    ]
    .into_iter()
    .filter_map(|(flag, name)| mods.contains(flag).then_some(name))
    .collect::<Vec<_>>()
    .join(" ")
}
impl Preferences {
    fn error(&self, index: usize, message: &str) {
        self.ivars().errors.borrow()[index].setStringValue(&NSString::from_str(message));
    }
    fn end_recording(&self) {
        if let Some(index) = self.ivars().recording.take() {
            self.error(index, "");
            self.ivars().buttons.borrow()[index].setTitle(&NSString::from_str(
                &shortcuts::symbols(&self.ivars().values.borrow()[index]),
            ));
            super::with_app(|app| {
                if let Some(keys) = app.ivars().shortcuts.borrow_mut().as_mut()
                    && let Err(error) = keys.resume()
                {
                    self.error(index, &error);
                }
            });
        }
    }
    fn record(&self, index: usize, event: &NSEvent) {
        let result = shortcuts::from_keycode(event.keyCode(), modifiers(event.modifierFlags()))
            .and_then(|key| {
                let value = shortcuts::canonical(key);
                if shortcuts::parse(&self.ivars().values.borrow()[1 - index]).ok() == Some(key) {
                    return Err("Esta combinação já está atribuída à outra captura.".into());
                }
                if shortcuts::system_conflict(key)? {
                    return Err(
                        "Combinação ativa no macOS. Escolha outra ou abra Ajustes de Teclado."
                            .into(),
                    );
                }
                Ok(value)
            });
        match result {
            Ok(value) => {
                self.ivars().values.borrow_mut()[index] = value;
                self.end_recording();
            }
            Err(error) => {
                self.error(index, &error);
                self.ivars().buttons.borrow()[index]
                    .setTitle(&NSString::from_str("Tente outra combinação…"));
            }
        }
    }
    fn button(
        &self,
        title: &str,
        action: objc2::runtime::Sel,
        frame: objc2_foundation::NSRect,
    ) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(self),
                Some(action),
                self.mtm(),
            )
        };
        button.setFrame(frame);
        button.setBezelStyle(NSBezelStyle::Push);
        self.contentView().unwrap().addSubview(&button);
        button
    }
}

fn build(app: &App) -> Retained<Preferences> {
    let mtm = app.mtm();
    let (screen, region) = super::shortcut_texts();
    let window = Preferences::alloc(mtm).set_ivars(PreferencesIvars {
        values: RefCell::new([screen, region]),
        ..Default::default()
    });
    let window: Retained<Preferences> = unsafe {
        msg_send![super(window), initWithContentRect: rect(0.0,0.0,640.0,440.0), styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable, backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str("Atalhos de captura"));
    window.setDelegate(Some(ProtocolObject::from_ref(&*window)));
    let content: Retained<PreferencesContent> = unsafe {
        msg_send![PreferencesContent::alloc(mtm), initWithFrame: rect(0.0,0.0,640.0,440.0)]
    };
    window.setContentView(Some(&content));
    let title = label(
        "Seu print, a uma combinação de distância",
        rect(28.0, 383.0, 584.0, 30.0),
        mtm,
    );
    title.setFont(Some(&NSFont::boldSystemFontOfSize(21.0)));
    title.setTextColor(Some(&NSColor::labelColor()));
    content.addSubview(&title);
    content.addSubview(&label(
        "Clique no atalho e pressione as teclas que deseja usar.",
        rect(28.0, 351.0, 584.0, 23.0),
        mtm,
    ));
    for (i, (title, description)) in [
        ("Tela principal", "Captura todo o monitor principal."),
        (
            "Região / janela",
            "Selecione uma área ou pressione Espaço para uma janela.",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 291.0 - i as f64 * 112.0;
        let heading = label(title, rect(28.0, y, 270.0, 24.0), mtm);
        heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
        heading.setTextColor(Some(&NSColor::labelColor()));
        content.addSubview(&heading);
        let description = label(description, rect(28.0, y - 36.0, 310.0, 34.0), mtm);
        description.setMaximumNumberOfLines(2);
        if let Some(cell) = description.cell() {
            cell.setWraps(true);
        }
        content.addSubview(&description);
        let button = window.button(
            &shortcuts::symbols(&window.ivars().values.borrow()[i]),
            sel!(recordShortcut:),
            rect(352.0, y - 8.0, 260.0, 36.0),
        );
        button.setTag(i as isize);
        button.setAccessibilityLabel(Some(&NSString::from_str(&format!("Atalho: {title}"))));
        button.setToolTip(Some(&NSString::from_str(
            "Gravar atalho. Escape cancela; Tab muda de campo.",
        )));
        window.ivars().buttons.borrow_mut().push(button);
        let error = label("", rect(28.0, y - 75.0, 584.0, 38.0), mtm);
        error.setMaximumNumberOfLines(3);
        if let Some(cell) = error.cell() {
            cell.setWraps(true);
        }
        error.setTextColor(Some(&NSColor::systemRedColor()));
        content.addSubview(&error);
        window.ivars().errors.borrow_mut().push(error);
    }
    content.addSubview(&label(
        "Use ⌘ com ⇧, ⌥ ou ⌃ e uma letra ou número. Esc cancela a gravação.",
        rect(28.0, 78.0, 584.0, 22.0),
        mtm,
    ));
    window.button(
        "Ajustes de Teclado…",
        sel!(openSettings:),
        rect(24.0, 23.0, 202.0, 32.0),
    );
    let cancel = window.button("Cancelar", sel!(cancel:), rect(384.0, 23.0, 106.0, 32.0));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    let apply = window.button("Aplicar", sel!(apply:), rect(502.0, 23.0, 110.0, 32.0));
    apply.setKeyEquivalent(&NSString::from_str("\r"));
    window.center();
    window
}

pub fn show(app: &App) {
    let mtm = app.mtm();
    let window = build(app);
    super::activate(mtm);
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).runModalForWindow(&window);
    window.end_recording();
    window.setDelegate(None);
    window.orderOut(None);
    window.close();
}

/// Exercise the recorder without registering hotkeys or changing user defaults.
pub fn verify(app: &App, output: &std::path::Path) {
    let window = build(app);
    let original = window.ivars().values.borrow().clone();
    let button = window.ivars().buttons.borrow()[0].clone();
    window.start_recording(sel!(recordShortcut:), &button);
    assert_eq!(window.ivars().recording.get(), Some(0));
    window.end_recording();
    assert_eq!(*window.ivars().values.borrow(), original);
    assert_eq!(window.ivars().recording.get(), None);
    for (name, appearance) in [
        ("light", unsafe { NSAppearanceNameAqua }),
        ("dark", unsafe { NSAppearanceNameDarkAqua }),
    ] {
        window.setAppearance(NSAppearance::appearanceNamed(appearance).as_deref());
        window.orderFront(None);
        window.displayIfNeeded();
        let content = window.contentView().unwrap();
        content.display();
        // Normalize snapshots to 8-bit RGBA; native cache bitmaps may use HDR formats.
        let bitmap = super::render::new_bitmap(640, 440).unwrap();
        content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
        let data = super::render::png(&bitmap).unwrap();
        std::fs::write(output.join(format!("shortcuts-{name}.png")), unsafe {
            data.as_bytes_unchecked()
        })
        .unwrap();
    }
    window.setDelegate(None);
    window.close();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modifier_preview_ignores_caps_lock() {
        let flags = NSEventModifierFlags::Command
            | NSEventModifierFlags::Shift
            | NSEventModifierFlags::CapsLock;
        assert_eq!(modifiers(flags), Modifiers::SUPER | Modifiers::SHIFT);
        assert_eq!(modifier_symbols(modifiers(flags)), "⇧ ⌘");
    }
}
