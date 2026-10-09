//! Native, read-only viewer for local OpenTelemetry files.
use super::{App, editor::label, render::rect};
use crate::logs::{self, Filter, Snapshot};
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSDateFormatter, NSObjectNSDelayedPerforming,
    NSObjectProtocol, NSSize, NSString, NSURL,
};
use std::cell::RefCell;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct LogsContent;
    unsafe impl NSObjectProtocol for LogsContent {}
    impl LogsContent {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: objc2_foundation::NSRect) {
            NSColor::windowBackgroundColor().setFill();
            NSBezierPath::bezierPathWithRect(self.bounds()).fill();
        }
    }
);

#[derive(Default)]
pub struct LogsIvars {
    snapshot: RefCell<Snapshot>,
    search: RefCell<Option<Retained<NSTextField>>>,
    filter: RefCell<Option<Retained<NSPopUpButton>>>,
    text: RefCell<Option<Retained<NSTextView>>>,
    summary: RefCell<Option<Retained<NSTextField>>>,
}

define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[ivars = LogsIvars]
    struct LogsWindow;
    unsafe impl NSObjectProtocol for LogsWindow {}
    unsafe impl NSWindowDelegate for LogsWindow {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _window: &NSWindow) -> bool { self.dismiss(sel!(closeLogs:), None); false }
    }
    impl LogsWindow {
        #[unsafe(method(closeLogs:))]
        fn dismiss(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
        #[unsafe(method(filterLogs:))]
        fn filter_logs(&self, _sender: Option<&AnyObject>) { self.render(); }
        #[unsafe(method(refreshLogs:))]
        fn refresh(&self, _sender: Option<&AnyObject>) {
            let snapshot = match logs::directories() {
                Ok(directories) => logs::load(&directories),
                Err(e) => Snapshot { issues: vec![e.to_string()], ..Snapshot::default() },
            };
            *self.ivars().snapshot.borrow_mut() = snapshot;
            self.render();
        }
        #[unsafe(method(openLogsFolder:))]
        fn open_folder(&self, _sender: Option<&AnyObject>) {
            if let Ok(directories) = logs::directories() {
                if let Some(path) = directories.iter().find(|path| path.is_dir()) {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    NSWorkspace::sharedWorkspace().openURL(&url);
                } else {
                    super::alert(self.mtm(), "Pasta de logs indisponível", "Nenhuma pasta de logs foi criada. Use Atualizar para consultar eventuais falhas de leitura.", &["OK"]);
                }
            }
        }
    }
);

impl LogsWindow {
    fn render(&self) {
        let filter = match self
            .ivars()
            .filter
            .borrow()
            .as_ref()
            .unwrap()
            .indexOfSelectedItem()
        {
            1 => Filter::Errors,
            2 => Filter::Warnings,
            3 => Filter::Operations,
            _ => Filter::All,
        };
        let query = self
            .ivars()
            .search
            .borrow()
            .as_ref()
            .unwrap()
            .stringValue()
            .to_string();
        let snapshot = self.ivars().snapshot.borrow();
        let entries: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| e.matches(filter, &query))
            .collect();
        let errors = snapshot.entries.iter().filter(|e| e.is_error()).count();
        let formatter = NSDateFormatter::new();
        formatter.setDateFormat(Some(&NSString::from_str("dd/MM/yyyy HH:mm:ss.SSS")));
        let mut text = String::new();
        if !snapshot.issues.is_empty() {
            text.push_str("Não foi possível ler parte do histórico:\n");
            text.push_str(&snapshot.issues.join("\n"));
            text.push_str("\n\n");
        }
        if snapshot.skipped > 0 {
            text.push_str(&format!(
                "{} registros inválidos foram ignorados.\n\n",
                snapshot.skipped
            ));
        }
        if entries.is_empty() {
            text.push_str(if snapshot.entries.is_empty() { "Nenhum registro disponível.\nUse o aplicativo e clique em Atualizar para consultar os eventos coletados." } else { "Nenhum registro corresponde ao filtro ou à busca.\nSelecione Todos ou tente outro termo." });
        }
        for entry in &entries {
            let date =
                NSDate::dateWithTimeIntervalSince1970(entry.timestamp_ns as f64 / 1_000_000_000.0);
            text.push_str(&entry.details(&formatter.stringFromDate(&date).to_string()));
            text.push_str("\n────────────────────────────────────────────────────────\n\n");
        }
        let note = if snapshot.limited {
            " · histórico recente limitado"
        } else {
            ""
        };
        let summary = format!(
            "{} de {} registros · {} com erro{} · mais recentes primeiro",
            entries.len(),
            snapshot.entries.len(),
            errors,
            note
        );
        self.ivars()
            .summary
            .borrow()
            .as_ref()
            .unwrap()
            .setStringValue(&NSString::from_str(&summary));
        self.ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .setString(&NSString::from_str(&text));
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

fn build(mtm: MainThreadMarker, snapshot: Snapshot) -> Retained<LogsWindow> {
    let window = unsafe {
        let allocated = LogsWindow::alloc(mtm).set_ivars(LogsIvars::default());
        msg_send![super(allocated), initWithContentRect: rect(0.0,0.0,900.0,640.0), styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable, backing: NSBackingStoreType::Buffered, defer: false]
    };
    let window: Retained<LogsWindow> = window;
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str("RSX — Logs locais"));
    window.setContentMinSize(NSSize::new(900.0, 640.0));
    window.setDelegate(Some(ProtocolObject::from_ref(&*window)));
    let content: Retained<LogsContent> =
        unsafe { msg_send![LogsContent::alloc(mtm), initWithFrame: rect(0.0,0.0,900.0,640.0)] };
    window.setContentView(Some(&content));
    let heading = label("Logs do RSX", rect(24.0, 590.0, 850.0, 30.0), mtm);
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
    heading.setTextColor(Some(&NSColor::labelColor()));
    heading.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&heading);
    let subtitle = label(
        "Histórico salvo neste Mac. Filtre os registros ou busque uma mensagem, operação ou trace.",
        rect(24.0, 562.0, 850.0, 22.0),
        mtm,
    );
    subtitle.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&subtitle);

    let filter = {
        NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(24.0, 518.0, 175.0, 32.0),
            false,
        )
    };
    for title in ["Todos", "Erros", "Avisos", "Operações"] {
        filter.addItemWithTitle(&NSString::from_str(title));
    }
    unsafe {
        filter.setTarget(Some(&*window));
    }
    unsafe {
        filter.setAction(Some(sel!(filterLogs:)));
    }
    filter.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    filter.setAccessibilityLabel(Some(&NSString::from_str("Filtrar logs por tipo")));
    content.addSubview(&filter);
    *window.ivars().filter.borrow_mut() = Some(filter);

    let search = NSTextField::new(mtm);
    search.setFrame(rect(210.0, 521.0, 390.0, 28.0));
    search.setPlaceholderString(Some(&NSString::from_str("Buscar nos logs…")));
    unsafe {
        search.setTarget(Some(&*window));
    }
    unsafe {
        search.setAction(Some(sel!(filterLogs:)));
    }
    search.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    search.setAccessibilityLabel(Some(&NSString::from_str(
        "Buscar nos logs; pressione Enter ou Buscar",
    )));
    content.addSubview(&search);
    *window.ivars().search.borrow_mut() = Some(search);
    for (title, action, x, width) in [
        ("Buscar", sel!(filterLogs:), 609.0, 100.0),
        ("Atualizar", sel!(refreshLogs:), 718.0, 158.0),
    ] {
        let button = window.button(title, action, rect(x, 518.0, width, 32.0));
        button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    }
    let summary = label("", rect(24.0, 480.0, 852.0, 26.0), mtm);
    summary.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewWidthSizable,
    );
    content.addSubview(&summary);
    *window.ivars().summary.borrow_mut() = Some(summary);

    let scroll =
        { NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(24.0, 68.0, 852.0, 400.0)) };
    scroll.setHasVerticalScroller(true);
    scroll.setBorderType(NSBorderType::BezelBorder);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let text = NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 830.0, 400.0));
    text.setEditable(false);
    text.setSelectable(true);
    text.setRichText(false);
    text.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(12.0, 0.0)));
    text.setTextColor(Some(&NSColor::labelColor()));
    text.setBackgroundColor(&NSColor::textBackgroundColor());
    text.setTextContainerInset(NSSize::new(12.0, 12.0));
    text.setMinSize(NSSize::new(0.0, 400.0));
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    if let Some(container) = unsafe { text.textContainer() } {
        container.setContainerSize(NSSize::new(830.0, f64::MAX));
        container.setWidthTracksTextView(true);
    }
    text.setAccessibilityLabel(Some(&NSString::from_str(
        "Registros locais, mais recentes primeiro",
    )));
    scroll.setDocumentView(Some(&text));
    content.addSubview(&scroll);
    *window.ivars().text.borrow_mut() = Some(text);
    window.button(
        "Abrir pasta de logs",
        sel!(openLogsFolder:),
        rect(24.0, 20.0, 180.0, 32.0),
    );
    let close = window.button("Fechar", sel!(closeLogs:), rect(756.0, 20.0, 120.0, 32.0));
    close.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    close.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    *window.ivars().snapshot.borrow_mut() = snapshot;
    window.render();
    window.center();
    window
}

pub fn show(app: &App) {
    let window = build(app.mtm(), Snapshot::default());
    window.refresh(sel!(refreshLogs:), None);
    super::activate(app.mtm());
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(app.mtm()).runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    window.close();
}

/// Exercise the native view with synthetic records only, without reading user logs.
pub fn verify(mtm: MainThreadMarker, output: &std::path::Path) {
    use crate::logs::Entry;
    use serde_json::json;
    let records = [
        json!({"kind":"log", "body":"operation.failed", "severity":"ERROR", "attributes":{"error.title":"Falha ao salvar", "error.message":"Não foi possível gravar o PNG: permissão negada."}, "trace_id":"a42fc59bd046c6d039f78242191588aa", "span_id":"c78e33d9bac42b10", "process.pid":1234}),
        json!({"kind":"span", "name":"image.export", "duration_ms":12.85, "status":"Ok", "trace_id":"a42fc59bd046c6d039f78242191588aa", "span_id":"a583d9bd8305c2b1"}),
        json!({"kind":"log", "body":"capture.permission_denied", "severity":"WARN", "attributes":{}}),
        json!({"kind":"log", "body":"capture.succeeded", "severity":"INFO", "attributes":{"mode":"Region"}}),
    ];
    let entries = records
        .into_iter()
        .enumerate()
        .map(|(index, record)| Entry {
            timestamp_ns: 1_790_000_000_000_000_000 - index as u128 * 1_000_000_000,
            record,
        })
        .collect();
    let window = build(
        mtm,
        Snapshot {
            entries,
            ..Snapshot::default()
        },
    );
    assert!(!window.ivars().text.borrow().as_ref().unwrap().isEditable());
    assert!(
        window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .isSelectable()
    );
    assert!(
        window
            .ivars()
            .summary
            .borrow()
            .as_ref()
            .unwrap()
            .stringValue()
            .to_string()
            .starts_with("4 de 4")
    );
    window
        .ivars()
        .filter
        .borrow()
        .as_ref()
        .unwrap()
        .selectItemAtIndex(1);
    window.filter_logs(sel!(filterLogs:), None);
    assert!(
        window
            .ivars()
            .summary
            .borrow()
            .as_ref()
            .unwrap()
            .stringValue()
            .to_string()
            .starts_with("1 de 4")
    );
    assert!(
        !window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .string()
            .to_string()
            .contains("capture.succeeded")
    );
    window
        .ivars()
        .filter
        .borrow()
        .as_ref()
        .unwrap()
        .selectItemAtIndex(0);
    window
        .ivars()
        .search
        .borrow()
        .as_ref()
        .unwrap()
        .setStringValue(&NSString::from_str("PERMISSÃO NEGADA"));
    window.filter_logs(sel!(filterLogs:), None);
    assert!(
        window
            .ivars()
            .summary
            .borrow()
            .as_ref()
            .unwrap()
            .stringValue()
            .to_string()
            .starts_with("1 de 4")
    );
    window
        .ivars()
        .search
        .borrow()
        .as_ref()
        .unwrap()
        .setStringValue(&NSString::from_str(""));
    window.filter_logs(sel!(filterLogs:), None);
    for (name, appearance) in [
        ("light", unsafe { NSAppearanceNameAqua }),
        ("dark", unsafe { NSAppearanceNameDarkAqua }),
    ] {
        window.setAppearance(NSAppearance::appearanceNamed(appearance).as_deref());
        window.orderFront(None);
        window.displayIfNeeded();
        let content = window.contentView().unwrap();
        content.display();
        let bitmap = super::render::new_bitmap(900, 640).unwrap();
        content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
        let data = super::render::png(&bitmap).unwrap();
        std::fs::write(output.join(format!("logs-{name}.png")), unsafe {
            data.as_bytes_unchecked()
        })
        .unwrap();
    }
    *window.ivars().snapshot.borrow_mut() = Snapshot::default();
    window.render();
    assert!(
        window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .string()
            .to_string()
            .contains("Nenhum registro disponível")
    );
    // Use the modal run loop rather than dispatch: this harness can itself run
    // inside a main-queue callback, where another queued callback cannot execute.
    let modes = NSArray::from_slice(&[unsafe { NSModalPanelRunLoopMode }]);
    unsafe {
        window.performSelector_withObject_afterDelay_inModes(
            sel!(performClose:),
            None,
            0.01,
            &modes,
        );
    }
    NSApplication::sharedApplication(mtm).runModalForWindow(&window);
    window.setDelegate(None);
    window.close();
}
