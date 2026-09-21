mod capture;
mod color_picker;
mod diagnostics;
mod editor;
mod preferences;
mod render;
mod shortcuts;

use crate::{
    capture::{CaptureBackend, CaptureMode, CaptureOutcome},
    document::Tool,
};
use capture::{MacCapture, Session};
use dispatch2::DispatchQueue;
use editor::Editor;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL, NSUserDefaults,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

thread_local! { static APP: RefCell<Option<Retained<App>>> = const { RefCell::new(None) }; }

pub fn with_app(f: impl FnOnce(&App)) {
    let app = APP.with(|slot| slot.borrow().clone());
    if let Some(app) = app {
        f(&app);
    }
}

pub fn activate(mtm: MainThreadMarker) {
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

pub fn alert(mtm: MainThreadMarker, title: &str, message: &str, buttons: &[&str]) -> isize {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    activate(mtm);
    alert.runModal()
}

pub struct AppIvars {
    session: Session,
    editor: RefCell<Option<Rc<Editor>>>,
    status: RefCell<Option<Retained<NSStatusItem>>>,
    shortcuts: RefCell<Option<shortcuts::Shortcuts>>,
    capturing: Cell<bool>,
    modal: Cell<bool>,
    screen_access_requested: Cell<bool>,
    diagnostics_mode: bool,
}

// SAFETY: AppKit delegates and action targets are main-thread-only. The app
// retains this delegate for its entire event loop; windows use weak delegates.
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppIvars]
    pub struct App;
    unsafe impl NSObjectProtocol for App {}
    unsafe impl NSApplicationDelegate for App {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_launch(&self, _notification: &NSNotification) {
            self.install_menus();
            if self.ivars().diagnostics_mode { return; }
            let defaults = NSUserDefaults::standardUserDefaults();
            if !defaults.boolForKey(&NSString::from_str("onboardingComplete")) {
                self.ivars().modal.set(true);
                let result = alert(self.mtm(), "Bem-vindo ao RXS", "O RXS vive na barra de menus e usa a seleção de captura do macOS.\n\nPara usar Command+Shift+3 e Command+Shift+4, desative essas duas combinações em Ajustes do Sistema → Teclado → Atalhos de Teclado → Capturas de Tela. Mantenha Command+Shift+5 ativado.\n\nDepois escolha “Atalhos…” no menu do RXS. Você também pode usar outras combinações ou capturar pelo menu.", &["Configurar atalhos…", "Usar pelo menu"]);
                defaults.setBool_forKey(true, &NSString::from_str("onboardingComplete"));
                self.ivars().modal.set(false);
                if result == 1000 { self.preferences(sel!(preferences:), None); }
            } else if defaults.boolForKey(&NSString::from_str("shortcutsEnabled")) {
                let (screen, region) = shortcut_texts();
                if let Err(error) = self.configure_shortcuts(&screen, &region) {
                    self.show_error("Atalhos indisponíveis", &error);
                }
            }
        }
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            if !self.ivars().capturing.get() && let Some(editor) = self.editor() { editor.show(); }
            true
        }
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            if self.ivars().capturing.get() || self.ivars().modal.get() { return NSApplicationTerminateReply::TerminateCancel; }
            if self.confirm_discard() {
                self.close_editor();
                NSApplicationTerminateReply::TerminateNow
            } else { NSApplicationTerminateReply::TerminateCancel }
        }
    }
    unsafe impl NSWindowDelegate for App {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _window: &NSWindow) -> bool {
            !self.ivars().modal.get() && self.confirm_discard()
        }
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            let number = self.editor().map(|editor| {
                editor.release_content();
                editor.window.windowNumber()
            });
            // Defer final view release until AppKit has finished its close callback.
            DispatchQueue::main().exec_async(move || with_app(|app| {
                if app.editor().is_some_and(|editor| Some(editor.window.windowNumber()) == number) {
                    app.ivars().editor.borrow_mut().take();
                    NSApplication::sharedApplication(app.mtm()).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                }
            }));
        }
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            if let Some(editor) = self.editor() {
                if editor.fit.get() { editor.fit_image(); } else { editor.center_image(); }
            }
        }
    }
    impl App {
        #[unsafe(method(captureScreen:))]
        fn capture_screen(&self, _sender: Option<&AnyObject>) { self.begin_capture(CaptureMode::Screen); }
        #[unsafe(method(captureRegion:))]
        fn capture_region(&self, _sender: Option<&AnyObject>) { self.begin_capture(CaptureMode::Region); }
        #[unsafe(method(captureWindow:))]
        fn capture_window(&self, _sender: Option<&AnyObject>) { self.begin_capture(CaptureMode::Window); }
        #[unsafe(method(showEditor:))]
        fn show_editor(&self, _sender: Option<&AnyObject>) { if let Some(editor) = self.editor() { editor.show(); } }
        #[unsafe(method(closeEditor:))]
        fn close_editor_action(&self, _sender: Option<&AnyObject>) {
            if !self.ivars().modal.get() && self.confirm_discard() { self.close_editor(); }
        }
        #[unsafe(method(selectTool:))]
        fn select_tool(&self, sender: &NSSegmentedControl) {
            if let Some(editor) = self.editor() { editor.canvas.set_tool(match sender.selectedSegment() { 1 => Tool::Rectangle, 2 => Tool::Freehand, _ => Tool::Arrow }); }
        }
        #[unsafe(method(toggleColors:))]
        fn toggle_colors(&self, _sender: Option<&AnyObject>) {
            if let Some(editor) = self.editor() { editor.toggle_colors(); }
        }
        #[unsafe(method(changeWidth:))]
        fn change_width(&self, sender: &NSPopUpButton) {
            if let Some(editor) = self.editor() { editor.canvas.ivars().document.borrow_mut().style.width = [2.0, 4.0, 8.0, 12.0][sender.indexOfSelectedItem().clamp(0, 3) as usize]; }
        }
        #[unsafe(method(undoDrawing:))]
        fn undo_drawing(&self, _sender: Option<&AnyObject>) {
            if let Some(editor) = self.editor() { editor.canvas.ivars().document.borrow_mut().undo(); editor.canvas.setNeedsDisplay(true); editor.refresh(); }
        }
        #[unsafe(method(redoDrawing:))]
        fn redo_drawing(&self, _sender: Option<&AnyObject>) {
            if let Some(editor) = self.editor() { editor.canvas.ivars().document.borrow_mut().redo(); editor.canvas.setNeedsDisplay(true); editor.refresh(); }
        }
        #[unsafe(method(zoomIn:))]
        fn zoom_in(&self, _sender: Option<&AnyObject>) { if let Some(e) = self.editor() { e.set_zoom(e.canvas.zoom() * 1.25); } }
        #[unsafe(method(zoomOut:))]
        fn zoom_out(&self, _sender: Option<&AnyObject>) { if let Some(e) = self.editor() { e.set_zoom(e.canvas.zoom() / 1.25); } }
        #[unsafe(method(fitImage:))]
        fn fit_image(&self, _sender: Option<&AnyObject>) { if let Some(e) = self.editor() { e.fit_image(); } }
        #[unsafe(method(actualSize:))]
        fn actual_size(&self, _sender: Option<&AnyObject>) { if let Some(e) = self.editor() { e.set_zoom(1.0); } }
        #[unsafe(method(copyImage:))]
        fn copy_image(&self, _sender: Option<&AnyObject>) {
            if self.ivars().modal.get() { return; }
            let Some(editor) = self.editor() else { return; };
            let result = autoreleasepool(|_| editor.copy_to(&NSPasteboard::generalPasteboard()));
            if let Err(error) = result { self.show_error("Falha ao copiar", &error); }
        }
        #[unsafe(method(saveImage:))]
        fn save_image(&self, _sender: Option<&AnyObject>) {
            if self.ivars().modal.get() { return; }
            let Some(editor) = self.editor() else { return; };
            self.ivars().modal.set(true);
            let panel = NSSavePanel::savePanel(self.mtm());
            panel.setTitle(Some(&NSString::from_str("Salvar captura anotada")));
            panel.setNameFieldStringValue(&NSString::from_str(&format!("RXS-{}.png", timestamp())));
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&objc2_foundation::NSArray::from_retained_slice(&[NSString::from_str("png")])));
            panel.setCanCreateDirectories(true);
            let result = if panel.runModal() == NSModalResponseOK {
                autoreleasepool(|_| -> Result<(), String> {
                    let url = panel.URL().ok_or("Nenhum destino selecionado.")?;
                    let path = url.path().ok_or("O destino precisa ser um arquivo local.")?.to_string();
                    let data = editor.canvas.export()?;
                    atomic_save(std::path::Path::new(&path), unsafe { data.as_bytes_unchecked() })?;
                    editor.canvas.mark_exported(); editor.refresh();
                    Ok(())
                })
            } else { Ok(()) };
            self.ivars().modal.set(false);
            if let Err(error) = result { self.show_error("Falha ao salvar", &error); }
        }
        #[unsafe(method(preferences:))]
        fn preferences(&self, _sender: Option<&AnyObject>) {
            if self.ivars().capturing.get() || self.ivars().modal.replace(true) { return; }
            preferences::show(self);
            self.ivars().modal.set(false);
        }
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) { NSApplication::sharedApplication(self.mtm()).terminate(None); }
    }
);

impl App {
    fn new(mtm: MainThreadMarker, session: Session, diagnostics_mode: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppIvars {
            session,
            editor: RefCell::new(None),
            status: RefCell::new(None),
            shortcuts: RefCell::new(None),
            capturing: Cell::new(false),
            modal: Cell::new(false),
            screen_access_requested: Cell::new(false),
            diagnostics_mode,
        });
        unsafe { msg_send![super(this), init] }
    }
    fn editor(&self) -> Option<Rc<Editor>> {
        self.ivars().editor.borrow().clone()
    }
    pub fn refresh(&self) {
        if let Some(editor) = self.editor() {
            editor.refresh();
        }
    }
    fn show_error(&self, title: &str, message: &str) {
        let was_modal = self.ivars().modal.replace(true);
        alert(self.mtm(), title, message, &["OK"]);
        self.ivars().modal.set(was_modal);
    }
    fn confirm_discard(&self) -> bool {
        if self.editor().is_none_or(|e| !e.canvas.needs_export()) {
            return true;
        }
        self.ivars().modal.set(true);
        let result = alert(
            self.mtm(),
            "Descartar esta captura?",
            "Esta versão ainda não foi copiada nem salva. Descartar remove a captura e seus desenhos.",
            &["Continuar editando", "Descartar"],
        );
        self.ivars().modal.set(false);
        result == 1001
    }
    fn close_editor(&self) {
        let editor = self.ivars().editor.borrow_mut().take();
        if let Some(editor) = editor {
            editor.window.setDelegate(None);
            editor.release_content();
            editor.window.close();
        }
        NSApplication::sharedApplication(self.mtm())
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
    fn configure_shortcuts(&self, screen: &str, region: &str) -> Result<(), String> {
        let mut slot = self.ivars().shortcuts.borrow_mut();
        if slot.is_none() {
            *slot = Some(shortcuts::Shortcuts::new()?);
        }
        slot.as_mut().unwrap().configure(screen, region)
    }
    fn begin_capture(&self, mode: CaptureMode) {
        if self.ivars().capturing.get() || self.ivars().modal.get() {
            return;
        }
        if !self.screen_access() {
            return;
        }
        if !self.confirm_discard() {
            return;
        }
        self.ivars().capturing.set(true);
        if let Some(editor) = self.editor() {
            editor.window.orderOut(None);
        }
        let backend = MacCapture {
            root: self.ivars().session.root.clone(),
        };
        std::thread::spawn(move || {
            // Allow the window server to remove the menu/editor before capture.
            std::thread::sleep(std::time::Duration::from_millis(180));
            let outcome = backend.capture(mode);
            DispatchQueue::main().exec_async(move || {
                autoreleasepool(|_| with_app(|app| app.finish_capture(outcome)))
            });
        });
    }
    fn screen_access(&self) -> bool {
        if objc2_core_graphics::CGPreflightScreenCaptureAccess() {
            return true;
        }
        self.ivars().modal.set(true);
        let granted = if self.ivars().screen_access_requested.replace(true) {
            false
        } else {
            objc2_core_graphics::CGRequestScreenCaptureAccess()
        };
        if !granted {
            let choice = alert(
                self.mtm(),
                "Reabra o RXS após autorizar",
                "Autorize o RXS em Ajustes do Sistema → Privacidade e Segurança → Gravação de Tela. O macOS pode só aplicar essa alteração depois que o app é encerrado.\n\nSe o RXS já estiver ativado na lista, desative e ative novamente. Se ainda não funcionar, remova a entrada antiga e adicione a cópia do RXS que você está usando.",
                &["Abrir Ajustes e encerrar RXS", "Agora não"],
            );
            if choice == 1000 {
                open_settings(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                );
                self.ivars().modal.set(false);
                NSApplication::sharedApplication(self.mtm()).terminate(None);
                return false;
            }
        }
        self.ivars().modal.set(false);
        granted
    }
    fn finish_capture(&self, outcome: CaptureOutcome) {
        self.ivars().capturing.set(false);
        match outcome {
            CaptureOutcome::Success(artifact) => {
                // Discard was already authorized before capture. Free the old
                // bitmap before decoding the new one to avoid overlapping peaks.
                self.close_editor();
                match Editor::new(self, artifact) {
                    Ok(editor) => {
                        let editor = Rc::new(editor);
                        *self.ivars().editor.borrow_mut() = Some(editor.clone());
                        let copied = if self.ivars().diagnostics_mode {
                            let board = NSPasteboard::pasteboardWithUniqueName();
                            let result = editor.copy_to(&board);
                            unsafe {
                                let _: () = msg_send![&*board, releaseGlobally];
                            }
                            result
                        } else {
                            editor.copy_to(&NSPasteboard::generalPasteboard())
                        };
                        editor.show();
                        if let Err(error) = copied {
                            self.show_error(
                                "Captura aberta, mas não copiada",
                                &format!("{error} Tente novamente pelo botão Copiar."),
                            );
                        }
                    }
                    Err(error) => self.show_error("Falha ao abrir captura", &error),
                }
            }
            CaptureOutcome::Cancelled => {
                if let Some(editor) = self.editor() {
                    editor.show();
                }
            }
            CaptureOutcome::Error(error) => {
                if let Some(editor) = self.editor() {
                    editor.show();
                }
                self.show_error("Falha na captura", &error);
            }
        }
    }
    fn item(
        &self,
        menu: &NSMenu,
        title: &str,
        action: objc2::runtime::Sel,
        key: &str,
        shift: bool,
    ) {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        // System dialogs need standard editing actions on the responder chain.
        let responder_action = [
            sel!(copy:),
            sel!(cut:),
            sel!(paste:),
            sel!(selectAll:),
            sel!(undo:),
            sel!(redo:),
        ]
        .contains(&action);
        unsafe {
            item.setTarget(if responder_action { None } else { Some(self) });
        }
        if !key.is_empty() {
            item.setKeyEquivalentModifierMask(if shift {
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift
            } else {
                NSEventModifierFlags::Command
            });
        }
        menu.addItem(&item);
    }
    fn install_menus(&self) {
        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        let main = NSMenu::new(mtm);
        let root = NSMenuItem::new(mtm);
        root.setTitle(&NSString::from_str("RXS"));
        let app_menu = NSMenu::new(mtm);
        app_menu.setTitle(&NSString::from_str("RXS"));
        self.item(&app_menu, "Atalhos…", sel!(preferences:), ",", false);
        self.item(&app_menu, "Sair do RXS", sel!(quit:), "q", false);
        root.setSubmenu(Some(&app_menu));
        main.addItem(&root);
        let file = NSMenuItem::new(mtm);
        file.setTitle(&NSString::from_str("Captura"));
        let file_menu = NSMenu::new(mtm);
        self.item(
            &file_menu,
            "Capturar tela principal",
            sel!(captureScreen:),
            "",
            false,
        );
        self.item(
            &file_menu,
            "Capturar região…",
            sel!(captureRegion:),
            "",
            false,
        );
        self.item(
            &file_menu,
            "Capturar janela…",
            sel!(captureWindow:),
            "",
            false,
        );
        self.item(&file_menu, "Salvar PNG…", sel!(saveImage:), "s", false);
        self.item(&file_menu, "Fechar editor", sel!(closeEditor:), "w", false);
        file.setSubmenu(Some(&file_menu));
        main.addItem(&file);
        let edit = NSMenuItem::new(mtm);
        edit.setTitle(&NSString::from_str("Editar"));
        let edit_menu = NSMenu::new(mtm);
        self.item(&edit_menu, "Desfazer", sel!(undo:), "z", false);
        self.item(&edit_menu, "Refazer", sel!(redo:), "z", true);
        self.item(&edit_menu, "Recortar", sel!(cut:), "x", false);
        self.item(&edit_menu, "Copiar", sel!(copy:), "c", false);
        self.item(&edit_menu, "Colar", sel!(paste:), "v", false);
        self.item(&edit_menu, "Selecionar tudo", sel!(selectAll:), "a", false);
        edit.setSubmenu(Some(&edit_menu));
        main.addItem(&edit);
        app.setMainMenu(Some(&main));
        let menu = NSMenu::new(mtm);
        self.item(
            &menu,
            "Capturar tela principal",
            sel!(captureScreen:),
            "",
            false,
        );
        self.item(&menu, "Capturar região…", sel!(captureRegion:), "", false);
        self.item(&menu, "Capturar janela…", sel!(captureWindow:), "", false);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.item(&menu, "Mostrar editor", sel!(showEditor:), "", false);
        self.item(&menu, "Atalhos…", sel!(preferences:), "", false);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.item(&menu, "Sair do RXS", sel!(quit:), "", false);
        let status = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = status.button(mtm) {
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("viewfinder"),
                Some(&NSString::from_str("RXS — Capturas de tela")),
            ) {
                image.setTemplate(true);
                button.setImage(Some(&image));
            } else {
                button.setTitle(&NSString::from_str("RXS"));
            }
            button.setToolTip(Some(&NSString::from_str("RXS — Capturas de tela")));
        }
        status.setMenu(Some(&menu));
        *self.ivars().status.borrow_mut() = Some(status);
    }
}

fn shortcut_texts() -> (String, String) {
    let defaults = NSUserDefaults::standardUserDefaults();
    (
        defaults
            .stringForKey(&NSString::from_str("screenShortcut"))
            .map_or_else(|| "Command+Shift+3".into(), |s| s.to_string()),
        defaults
            .stringForKey(&NSString::from_str("regionShortcut"))
            .map_or_else(|| "Command+Shift+4".into(), |s| s.to_string()),
    )
}
fn open_settings(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn atomic_save(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Pasta de destino inválida.")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path)
        .map_err(|e| format!("Não foi possível salvar: {}", e.error))?;
    Ok(())
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("RXS must start on the main thread");
    // AppKit uses the process name for the application menu when launched directly.
    objc2_foundation::NSProcessInfo::processInfo().setProcessName(&NSString::from_str("RXS"));
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--write-icon") {
        let path = args
            .get(2)
            .expect("--write-icon requires a destination PNG");
        autoreleasepool(|_| diagnostics::write_icon(std::path::Path::new(path)))
            .expect("write icon");
        return;
    }
    let session = match Session::open() {
        Ok(session) => session,
        Err(error) => {
            alert(mtm, "RXS", &error, &["OK"]);
            return;
        }
    };
    let diagnostics_mode = args
        .iter()
        .any(|arg| arg == "--diagnostics" || arg == "--demo");
    let delegate = App::new(mtm, session, diagnostics_mode);
    application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    APP.with(|slot| *slot.borrow_mut() = Some(delegate.clone()));
    GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }
        DispatchQueue::main().exec_async(move || {
            with_app(|app| {
                let position = app
                    .ivars()
                    .shortcuts
                    .borrow()
                    .as_ref()
                    .and_then(|keys| keys.active.iter().position(|key| key.id() == event.id));
                if let Some(position) = position {
                    app.begin_capture(if position == 0 {
                        CaptureMode::Screen
                    } else {
                        CaptureMode::Region
                    });
                }
            })
        });
    }));
    if diagnostics_mode {
        let demo = args.iter().any(|arg| arg == "--demo");
        DispatchQueue::main().exec_async(move || with_app(|app| diagnostics::run(app, demo)));
    }
    application.run();
    APP.with(|slot| slot.borrow_mut().take());
}
