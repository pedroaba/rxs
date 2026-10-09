//! Native, read-only viewer for local OpenTelemetry files.
use super::{App, editor::label, render::rect};
use crate::logs::{self, Entry, Filter, Snapshot};
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSDateFormatter, NSIndexSet, NSMutableAttributedString,
    NSNotification, NSObjectNSDelayedPerforming, NSObjectProtocol, NSRange, NSSize, NSString,
    NSURL,
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
            super::ui::color(super::ui::Role::Background).setFill();
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
    warnings: RefCell<Option<Retained<NSTextField>>>,
    table: RefCell<Option<Retained<NSTableView>>>,
    split: RefCell<Option<Retained<NSSplitView>>>,
    rows: RefCell<Vec<Entry>>,
    selected: RefCell<Option<Box<Entry>>>,
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
    unsafe impl NSTableViewDataSource for LogsWindow {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> isize { self.ivars().rows.borrow().len() as isize }
        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn row_value(&self, _table: &NSTableView, column: Option<&NSTableColumn>, row: isize) -> Option<Retained<AnyObject>> {
            self.object_value(column, row)
        }
    }
    unsafe impl NSControlTextEditingDelegate for LogsWindow {}
    unsafe impl NSTableViewDelegate for LogsWindow {
        #[unsafe(method(tableView:willDisplayCell:forTableColumn:row:))]
        unsafe fn display_cell(&self, _table: &NSTableView, cell: &AnyObject, _column: Option<&NSTableColumn>, _row: isize) {
            // Every column is created below with an NSTextFieldCell.
            let cell: &NSTextFieldCell = unsafe { &*(std::ptr::from_ref(cell).cast()) };
            cell.setHighlighted(false);
            cell.setDrawsBackground(false);
            cell.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
        }
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _notification: &NSNotification) { self.show_selected(); }
    }
    unsafe impl NSSplitViewDelegate for LogsWindow {
        #[unsafe(method(splitView:constrainMinCoordinate:ofSubviewAt:))]
        fn split_min(&self, _split: &NSSplitView, proposed: f64, _index: isize) -> f64 { proposed.max(320.0) }
        #[unsafe(method(splitView:constrainMaxCoordinate:ofSubviewAt:))]
        fn split_max(&self, split: &NSSplitView, proposed: f64, _index: isize) -> f64 { proposed.min(split.bounds().size.width - 280.0) }
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
    fn object_value(
        &self,
        column: Option<&NSTableColumn>,
        row: isize,
    ) -> Option<Retained<AnyObject>> {
        let rows = self.ivars().rows.borrow();
        let entry = rows.get(usize::try_from(row).ok()?)?;
        let id = column?.identifier().to_string();
        let value = match id.as_str() {
            "time" => self.date(entry, "HH:mm:ss"),
            "kind" => self.kind(entry).into(),
            _ => entry.record[if entry.is_operation() { "name" } else { "body" }]
                .as_str()
                .unwrap_or("Evento")
                .into(),
        };
        Some(NSString::from_str(&value).into_super().into_super())
    }
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
        let selected = self.ivars().selected.borrow().clone();
        let position = selected
            .as_ref()
            .and_then(|previous| {
                entries.iter().position(|entry| {
                    entry.timestamp_ns == previous.timestamp_ns && entry.record == previous.record
                })
            })
            .unwrap_or(0);
        *self.ivars().rows.borrow_mut() = entries.iter().map(|entry| (*entry).clone()).collect();
        let split = self.ivars().split.borrow().as_ref().unwrap().clone();
        let size = self.contentView().unwrap().bounds().size;
        let reserve = if snapshot.issues.is_empty() && snapshot.skipped == 0 && !snapshot.limited {
            0.0
        } else {
            36.0
        };
        split.setFrame(rect(
            24.0,
            68.0,
            size.width - 48.0,
            size.height - 240.0 - reserve,
        ));
        let mut warnings = snapshot.issues.join(" · ");
        if snapshot.skipped > 0 {
            warnings.push_str(&format!(
                " · {} registros inválidos ignorados",
                snapshot.skipped
            ));
        }
        if snapshot.limited {
            warnings
                .push_str(" · Histórico limitado. Os arquivos completos estão na pasta de logs.");
        }
        self.ivars()
            .warnings
            .borrow()
            .as_ref()
            .unwrap()
            .setStringValue(&NSString::from_str(warnings.trim_start_matches(" · ")));
        self.ivars()
            .warnings
            .borrow()
            .as_ref()
            .unwrap()
            .setToolTip(Some(&NSString::from_str(&warnings)));
        let empty = if snapshot.entries.is_empty() {
            if snapshot.issues.is_empty() {
                "Nenhum registro disponível.\nUse o aplicativo e clique em Atualizar."
            } else {
                "Não foi possível carregar o histórico.\nConfira o aviso acima e tente Atualizar."
            }
        } else {
            "Nenhum registro corresponde ao filtro ou à busca.\nSelecione Todos ou tente outro termo."
        };
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
        // Release snapshot borrows before AppKit asks its data source for rows.
        let has_rows = !entries.is_empty();
        drop(snapshot);
        let table = self.ivars().table.borrow().as_ref().unwrap().clone();
        table.reloadData();
        if has_rows {
            table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(position),
                false,
            );
            self.show_selected();
        } else {
            unsafe {
                table.deselectAll(None);
            }
            self.ivars().selected.borrow_mut().take();
            self.ivars()
                .text
                .borrow()
                .as_ref()
                .unwrap()
                .setString(&NSString::from_str(empty));
        }
    }
    fn kind(&self, entry: &Entry) -> &'static str {
        if entry.is_error() {
            "Erro"
        } else if entry.is_operation() {
            "Operação"
        } else if entry.record["severity"] == "WARN" {
            "Aviso"
        } else {
            "Informação"
        }
    }
    fn date(&self, entry: &Entry, format: &str) -> String {
        let formatter = NSDateFormatter::new();
        formatter.setDateFormat(Some(&NSString::from_str(format)));
        formatter
            .stringFromDate(&NSDate::dateWithTimeIntervalSince1970(
                entry.timestamp_ns as f64 / 1_000_000_000.0,
            ))
            .to_string()
    }
    fn show_selected(&self) {
        let row = self.ivars().table.borrow().as_ref().unwrap().selectedRow();
        let entry = usize::try_from(row)
            .ok()
            .and_then(|row| self.ivars().rows.borrow().get(row).cloned());
        let text = entry
            .as_ref()
            .map(|entry| entry.details(&self.date(entry, "dd/MM/yyyy HH:mm:ss.SSS")))
            .unwrap_or_else(|| "Selecione um registro para ver os detalhes.".into());
        *self.ivars().selected.borrow_mut() = entry.map(Box::new);
        let styled = NSMutableAttributedString::initWithString(
            NSMutableAttributedString::alloc(),
            &NSString::from_str(&text),
        );
        let entire = NSRange::new(0, text.encode_utf16().count());
        let paragraphs = NSMutableParagraphStyle::new();
        paragraphs.setParagraphSpacing(8.0);
        paragraphs.setLineSpacing(3.0);
        unsafe {
            styled.addAttribute_value_range(
                NSFontAttributeName,
                &NSFont::systemFontOfSize(13.0),
                entire,
            );
            styled.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &super::ui::color(super::ui::Role::Text),
                entire,
            );
            styled.addAttribute_value_range(NSParagraphStyleAttributeName, &paragraphs, entire);
        }
        let mut offset = 0;
        for (index, line) in text.split_inclusive('\n').enumerate() {
            let range = NSRange::new(offset, line.encode_utf16().count());
            let font = if index == 0 {
                NSFont::systemFontOfSize(11.0)
            } else if index == 1 {
                NSFont::boldSystemFontOfSize(17.0)
            } else if line.starts_with("Trace:") || line.starts_with("Span:") {
                NSFont::monospacedSystemFontOfSize_weight(11.0, 0.0)
            } else {
                NSFont::systemFontOfSize(13.0)
            };
            unsafe {
                styled.addAttribute_value_range(NSFontAttributeName, &font, range);
                if index == 0 || line.starts_with("Trace:") || line.starts_with("Span:") {
                    styled.addAttribute_value_range(
                        NSForegroundColorAttributeName,
                        &super::ui::color(super::ui::Role::Muted),
                        range,
                    );
                }
            }
            offset += range.length;
        }
        let text_view = self.ivars().text.borrow();
        if let Some(storage) = unsafe { text_view.as_ref().unwrap().textStorage() } {
            storage.setAttributedString(&styled);
        }
    }
    fn button(
        &self,
        title: &str,
        action: objc2::runtime::Sel,
        frame: objc2_foundation::NSRect,
    ) -> Retained<NSButton> {
        let button = super::ui::button(self.mtm(), title, self, action, frame);
        button.setFrame(frame);
        super::ui::style_button(&button);
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
    heading.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
    heading.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&heading);
    let subtitle = label(
        "Histórico salvo neste Mac. Filtre os registros ou busque uma mensagem, operação ou trace.",
        rect(24.0, 562.0, 850.0, 22.0),
        mtm,
    );
    subtitle.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&subtitle);

    let filter = super::ui::select(mtm, rect(24.0, 518.0, 175.0, 32.0));
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

    let search = super::ui::input(mtm, "", rect(210.0, 518.0, 390.0, 32.0));
    search.setFrame(rect(210.0, 518.0, 390.0, 32.0));
    search.setPlaceholderString(Some(&NSString::from_str("Buscar nos logs…")));
    unsafe {
        search.setTarget(Some(&*window));
    }
    unsafe {
        search.setAction(Some(sel!(filterLogs:)));
    }
    search.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewWidthSizable,
    );
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
        button.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewMinXMargin,
        );
    }
    let summary = label("", rect(24.0, 480.0, 852.0, 26.0), mtm);
    summary.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewWidthSizable,
    );
    content.addSubview(&summary);
    *window.ivars().summary.borrow_mut() = Some(summary);

    let scroll =
        { NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 0.0, 420.0, 364.0)) };
    scroll.setHasVerticalScroller(true);
    super::ui::round_scroll(&scroll);
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let text = NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 830.0, 400.0));
    text.setEditable(false);
    text.setSelectable(true);
    text.setRichText(true);
    text.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(13.0, 0.0)));
    text.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
    text.setBackgroundColor(&super::ui::color(super::ui::Role::Surface));
    text.setTextContainerInset(NSSize::new(20.0, 20.0));
    text.setMinSize(NSSize::new(0.0, 0.0));
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    if let Some(container) = unsafe { text.textContainer() } {
        container.setContainerSize(NSSize::new(830.0, f64::MAX));
        container.setWidthTracksTextView(true);
    }
    text.setAccessibilityLabel(Some(&NSString::from_str(
        "Detalhes do registro selecionado",
    )));
    scroll.setDocumentView(Some(&text));
    *window.ivars().text.borrow_mut() = Some(text);
    let warnings = label("", rect(24.0, 440.0, 852.0, 36.0), mtm);
    warnings.setMaximumNumberOfLines(2);
    if let Some(cell) = warnings.cell() {
        cell.setWraps(true);
    }
    warnings.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewWidthSizable,
    );
    content.addSubview(&warnings);
    *window.ivars().warnings.borrow_mut() = Some(warnings);
    let split = super::ui::split(mtm, rect(24.0, 68.0, 852.0, 364.0));
    split.setVertical(true);
    split.setDividerStyle(NSSplitViewDividerStyle::Thin);
    split.setDelegate(Some(ProtocolObject::from_ref(&*window)));
    split.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let list_scroll =
        NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 0.0, 432.0, 364.0));
    super::ui::round_scroll(&list_scroll);
    list_scroll.setFocusRingType(NSFocusRingType::None);
    list_scroll.setBorderType(NSBorderType::NoBorder);
    list_scroll.setHasVerticalScroller(true);
    list_scroll.setHasHorizontalScroller(true);
    list_scroll.setAutohidesScrollers(true);
    let table = super::ui::table(mtm, list_scroll.bounds());
    table.setRowHeight(44.0);
    table.setIntercellSpacing(NSSize::new(0.0, 0.0));
    table.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    table.setHeaderView(Some(&super::ui::table_header(mtm)));
    table.setBackgroundColor(&super::ui::color(super::ui::Role::Surface));
    table.setAllowsMultipleSelection(false);
    table.setAllowsColumnReordering(false);
    table.setColumnAutoresizingStyle(
        NSTableViewColumnAutoresizingStyle::LastColumnOnlyAutoresizingStyle,
    );
    table.setAccessibilityLabel(Some(&NSString::from_str(
        "Registros locais, mais recentes primeiro",
    )));
    for (id, title, width) in [
        ("time", "Horário", 86.0),
        ("kind", "Tipo", 118.0),
        ("message", "Evento / operação", 228.0),
    ] {
        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str(id));
        column.setTitle(&NSString::from_str(title));
        column.setWidth(width);
        column.setMinWidth(if id == "message" { 140.0 } else { width });
        column.setEditable(false);
        let cell = super::ui::table_cell(mtm, id);
        cell.setFont(Some(&NSFont::systemFontOfSize(super::ui::BODY)));
        cell.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
        unsafe {
            column.setDataCell(&cell);
        }
        table.addTableColumn(&column);
    }
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(&*window)));
        table.setDelegate(Some(ProtocolObject::from_ref(&*window)));
    }
    list_scroll.setDocumentView(Some(&table));
    *window.ivars().table.borrow_mut() = Some(table);
    split.addSubview(&list_scroll);
    split.addSubview(&scroll);
    split.adjustSubviews();
    content.addSubview(&split);
    *window.ivars().split.borrow_mut() = Some(split);
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
    window.setInitialFirstResponder(Some(window.ivars().search.borrow().as_ref().unwrap()));
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
    // Selection survives insertion of a newer record and falls back after removal.
    let table = window.ivars().table.borrow().as_ref().unwrap().clone();
    fn send_arrow(window: &NSWindow, table: &NSTableView, character: &str, code: u16) {
        let characters = NSString::from_str(character);
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown, objc2_foundation::NSPoint::new(0.0,0.0), NSEventModifierFlags::empty(), 0.0, window.windowNumber(), None, &characters, &characters, false, code,
        ).unwrap();
        table.keyDown(&event);
    }
    assert!(window.makeFirstResponder(Some(&table)));
    send_arrow(&window, &table, "\u{f701}", 125);
    assert_eq!(table.selectedRow(), 1);
    assert!(
        window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .string()
            .to_string()
            .contains("image.export")
    );
    send_arrow(&window, &table, "\u{f700}", 126);
    assert_eq!(table.selectedRow(), 0);
    window.makeFirstResponder(Some(window.ivars().search.borrow().as_ref().unwrap()));
    table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(1), false);
    window.show_selected();
    let original_entries = window.ivars().snapshot.borrow().entries.clone();
    window.ivars().snapshot.borrow_mut().entries.insert(
        0,
        Entry {
            timestamp_ns: 1_790_000_001_000_000_000,
            record: json!({"kind":"log", "body":"new.event", "severity":"INFO"}),
        },
    );
    window.render();
    assert_eq!(table.selectedRow(), 2);
    assert!(
        window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .string()
            .to_string()
            .contains("image.export")
    );
    window.ivars().snapshot.borrow_mut().entries.remove(2);
    window.render();
    assert_eq!(table.selectedRow(), 0);
    window.ivars().snapshot.borrow_mut().entries = original_entries;
    window.ivars().snapshot.borrow_mut().limited = true;
    window.ivars().snapshot.borrow_mut().issues =
        vec!["Um arquivo não pôde ser lido. Tente Atualizar.".into()];
    window.render();
    assert!(
        window
            .ivars()
            .warnings
            .borrow()
            .as_ref()
            .unwrap()
            .stringValue()
            .to_string()
            .contains("Histórico limitado")
    );
    window.ivars().snapshot.borrow_mut().limited = false;
    window.ivars().snapshot.borrow_mut().issues.clear();
    window.render();
    window
        .ivars()
        .search
        .borrow()
        .as_ref()
        .unwrap()
        .setStringValue(&NSString::from_str("sem-resultados"));
    window.render();
    assert_eq!(table.numberOfRows(), 0);
    assert!(
        window
            .ivars()
            .text
            .borrow()
            .as_ref()
            .unwrap()
            .string()
            .to_string()
            .contains("Nenhum registro corresponde")
    );
    window
        .ivars()
        .search
        .borrow()
        .as_ref()
        .unwrap()
        .setStringValue(&NSString::from_str(""));
    window.render();
    window.setContentSize(NSSize::new(1200.0, 800.0));
    assert!(
        window
            .ivars()
            .split
            .borrow()
            .as_ref()
            .unwrap()
            .frame()
            .size
            .width
            > 852.0
    );
    window.setContentSize(NSSize::new(900.0, 640.0));
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
        let search = window.ivars().search.borrow().as_ref().unwrap().clone();
        search.setStringValue(&NSString::from_str("capture.permission_denied"));
        content.display();
        content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
        let data = super::render::png(&bitmap).unwrap();
        std::fs::write(output.join(format!("logs-search-{name}.png")), unsafe {
            data.as_bytes_unchecked()
        })
        .unwrap();
        search.setStringValue(&NSString::from_str(""));
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
