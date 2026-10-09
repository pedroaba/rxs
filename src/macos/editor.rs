use super::color_picker::ColorPicker;
use super::render::{self, NativeImage, rect};
use crate::{
    capture::CaptureArtifact,
    document::{Annotation, Document, Point, Tool},
};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::*;
use objc2_core_graphics::CGContext;
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

pub static LIVE_CANVASES: AtomicUsize = AtomicUsize::new(0);

// Keep the image centered on each axis where it fits, without changing drawing coordinates.
define_class!(
    #[unsafe(super(NSClipView))]
    #[thread_kind = MainThreadOnly]
    pub struct CenteredClipView;
    unsafe impl NSObjectProtocol for CenteredClipView {}
    impl CenteredClipView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
        #[unsafe(method(constrainBoundsRect:))]
        fn constrain_bounds(&self, proposed: NSRect) -> NSRect {
            let mut bounds: NSRect = unsafe { msg_send![super(self), constrainBoundsRect: proposed] };
            if let Some(document) = self.documentView() {
                let frame = document.frame();
                if frame.size.width < bounds.size.width {
                    bounds.origin.x = frame.origin.x - (bounds.size.width - frame.size.width) / 2.0;
                }
                if frame.size.height < bounds.size.height {
                    bounds.origin.y = frame.origin.y - (bounds.size.height - frame.size.height) / 2.0;
                }
            }
            bounds
        }
    }
);

// Window fallback keeps copy available when toolbar controls own keyboard focus.
// Text fields handle copy first through the normal responder chain.
define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    struct EditorWindow;
    unsafe impl NSObjectProtocol for EditorWindow {}
    impl EditorWindow {
        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            super::with_app(|app| app.copy_image(sel!(copyImage:), None));
        }
    }
);

pub struct CanvasIvars {
    pub image: NativeImage,
    pub document: RefCell<Document>,
    draft: RefCell<Option<Annotation>>,
    zoom: Cell<f64>,
    pub _artifact: CaptureArtifact,
}

impl Drop for CanvasIvars {
    fn drop(&mut self) {
        LIVE_CANVASES.fetch_sub(1, Ordering::Relaxed);
    }
}

// SAFETY: The view and all its mutable state are confined to AppKit's main thread.
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = CanvasIvars]
    pub struct Canvas;
    unsafe impl NSObjectProtocol for Canvas {}
    impl Canvas {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first(&self) -> bool { true }
        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            super::with_app(|app| app.copy_image(sel!(copyImage:), None));
        }
        #[unsafe(method(undo:))]
        fn undo(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            super::with_app(|app| app.undo_drawing(sel!(undoDrawing:), None));
        }
        #[unsafe(method(redo:))]
        fn redo(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            super::with_app(|app| app.redo_drawing(sel!(redoDrawing:), None));
        }
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _rect: NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else { return; };
            let cg = context.CGContext();
            CGContext::save_g_state(Some(&cg));
            let zoom = self.ivars().zoom.get();
            CGContext::scale_ctm(Some(&cg), zoom, zoom);
            render::draw(&self.ivars().image, &self.ivars().document.borrow(), self.ivars().draft.borrow().as_ref());
            CGContext::restore_g_state(Some(&cg));
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() { window.makeFirstResponder(Some(self)); }
            let point = self.event_point(event);
            let doc = self.ivars().document.borrow();
            *self.ivars().draft.borrow_mut() = Some(Annotation::new(doc.tool, doc.style, point));
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(draft) = self.ivars().draft.borrow_mut().as_mut() { draft.update(self.event_point(event)); }
            self.setNeedsDisplay(true);
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if let Some(mut draft) = self.ivars().draft.borrow_mut().take() {
                draft.update(self.event_point(event));
                self.ivars().document.borrow_mut().push(draft);
            }
            self.setNeedsDisplay(true);
            super::with_app(|app| app.refresh());
        }
        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            self.ivars().draft.borrow_mut().take();
            self.setNeedsDisplay(true);
        }
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::crosshairCursor());
        }
    }
);

impl Canvas {
    pub fn new(
        mtm: MainThreadMarker,
        image: NativeImage,
        artifact: CaptureArtifact,
    ) -> Retained<Self> {
        LIVE_CANVASES.fetch_add(1, Ordering::Relaxed);
        let frame = rect(0.0, 0.0, image.width, image.height);
        let doc = Document::new(image.width, image.height);
        let this = Self::alloc(mtm).set_ivars(CanvasIvars {
            image,
            document: RefCell::new(doc),
            draft: RefCell::new(None),
            zoom: Cell::new(1.0),
            _artifact: artifact,
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
    fn event_point(&self, event: &NSEvent) -> Point {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        let doc = self.ivars().document.borrow();
        Point::new(p.x, p.y).to_image(self.ivars().zoom.get(), doc.width, doc.height)
    }
    pub fn zoom(&self) -> f64 {
        self.ivars().zoom.get()
    }
    pub fn set_zoom(&self, zoom: f64) {
        self.ivars().zoom.set(zoom);
        self.setFrameSize(NSSize::new(
            self.ivars().image.width * zoom,
            self.ivars().image.height * zoom,
        ));
        self.setNeedsDisplay(true);
    }
    pub fn set_tool(&self, tool: Tool) {
        self.ivars().document.borrow_mut().tool = tool;
    }
    pub fn export(&self) -> Result<Retained<objc2_foundation::NSData>, String> {
        let _operation = crate::telemetry::Operation::start("image.export");
        let result = render::export(&self.ivars().image, &self.ivars().document.borrow());
        match &result {
            Ok(data) => tracing::info!(bytes = data.length(), "image.exported"),
            Err(error) => crate::telemetry::error("Exportação", error),
        }
        result
    }
    pub fn needs_export(&self) -> bool {
        self.ivars().document.borrow().needs_export()
    }
    pub fn mark_exported(&self) {
        self.ivars().document.borrow_mut().mark_exported();
    }
}

pub struct Editor {
    pub window: Retained<NSWindow>,
    pub canvas: Retained<Canvas>,
    pub scroll: Retained<NSScrollView>,
    pub status: Retained<NSTextField>,
    pub undo: Retained<NSButton>,
    pub redo: Retained<NSButton>,
    color_picker: ColorPicker,
    color_button: Retained<NSButton>,
    pub fit: Cell<bool>,
    toolbar: Retained<NSView>,
    header: Retained<NSView>,
    board: Retained<NSView>,
    tools: Retained<NSSegmentedControl>,
    width: Retained<NSPopUpButton>,
    copy: Retained<NSButton>,
    save: Retained<NSButton>,
    metrics: Retained<NSTextField>,
    feedback: RefCell<String>,
}

pub fn button(
    title: &str,
    target: &super::App,
    action: objc2::runtime::Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = super::ui::button(target.mtm(), title, target, action, frame);
    button.setFrame(frame);
    super::ui::style_button(&button);
    button
}

pub fn label(text: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    super::ui::secondary_label(&label);
    label
}

impl Editor {
    /// AppKit may keep recently closed windows for animation/accessibility.
    /// Detach the bitmap-bearing view before closing, independently of that
    /// native window lifetime.
    pub fn release_content(&self) {
        self.color_picker.invalidate();
        self.window.makeFirstResponder(None);
        self.scroll.setDocumentView(None);
        self.window.setContentView(None);
    }
    pub fn new(app: &super::App, artifact: CaptureArtifact) -> Result<Self, String> {
        let mtm = app.mtm();
        let image = NativeImage::load(&artifact.path)?;
        let canvas = Canvas::new(mtm, image, artifact);
        let window = unsafe {
            let window: Retained<EditorWindow> = msg_send![EditorWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, 1040.0, 700.0), styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::Resizable, backing: NSBackingStoreType::Buffered, defer: false];
            Retained::into_super(window)
        };
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setContentMinSize(NSSize::new(760.0, 450.0));
        }
        window.setTitle(&NSString::from_str("RSX — Anotar captura"));
        window.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(app)));
        window.center();
        let content = super::ui::ChromeView::create(mtm, rect(0.0, 0.0, 1040.0, 700.0));
        window.setContentView(Some(&content));

        let toolbar = super::ui::ChromeView::create(mtm, rect(0.0, 644.0, 1040.0, 56.0));
        toolbar.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        content.addSubview(&toolbar);
        let header = super::ui::ChromeView::create(mtm, rect(0.0, 636.0, 1040.0, 64.0));
        content.addSubview(&header);
        let brand = label("RSX", rect(24.0, 22.0, 48.0, 24.0), mtm);
        brand.setFont(Some(&NSFont::boldSystemFontOfSize(18.0)));
        brand.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
        header.addSubview(&brand);
        let heading = label("Editor de captura", rect(88.0, 23.0, 240.0, 22.0), mtm);
        heading.setFont(Some(&NSFont::systemFontOfSize(15.0)));
        heading.setTextColor(Some(&super::ui::color(super::ui::Role::Text)));
        header.addSubview(&heading);
        let tools = super::ui::segments(
            mtm,
            &["Seta", "Retângulo", "Livre"],
            app,
            sel!(selectTool:),
            rect(16.0, 12.0, 224.0, 36.0),
        );
        tools.setFrame(rect(12.0, 15.0, 230.0, 28.0));
        tools.setSelectedSegment(0);
        tools.setFont(Some(&NSFont::systemFontOfSize(super::ui::BODY)));
        tools.setAccessibilityLabel(Some(&NSString::from_str("Ferramenta de anotação")));
        toolbar.addSubview(&tools);
        let color = button(
            "Cor",
            app,
            sel!(toggleColors:),
            rect(250.0, 15.0, 52.0, 27.0),
        );
        color.setAccessibilityLabel(Some(&NSString::from_str("Cor do traço")));
        color.setToolTip(Some(&NSString::from_str("Escolher cor do desenho")));
        toolbar.addSubview(&color);
        let width = super::ui::select(mtm, rect(332.0, 12.0, 84.0, 36.0));
        for title in ["2 px", "4 px", "8 px", "12 px"] {
            width.addItemWithTitle(&NSString::from_str(title));
        }
        width.selectItemAtIndex(1);
        unsafe {
            width.setTarget(Some(app));
            width.setAction(Some(sel!(changeWidth:)));
        }
        width.setAccessibilityLabel(Some(&NSString::from_str("Espessura do traço em pixels")));
        width.setToolTip(Some(&NSString::from_str("Espessura em pixels da imagem")));
        toolbar.addSubview(&width);
        let undo = button(
            "Desfazer",
            app,
            sel!(undoDrawing:),
            rect(408.0, 15.0, 90.0, 28.0),
        );
        let redo = button(
            "Refazer",
            app,
            sel!(redoDrawing:),
            rect(500.0, 15.0, 85.0, 28.0),
        );
        toolbar.addSubview(&undo);
        toolbar.addSubview(&redo);
        let copy = button(
            "Copiar ⌘C",
            app,
            sel!(copyImage:),
            rect(776.0, 15.0, 120.0, 28.0),
        );
        let save = button(
            "Salvar PNG…",
            app,
            sel!(saveImage:),
            rect(904.0, 15.0, 124.0, 28.0),
        );
        copy.setToolTip(Some(&NSString::from_str(
            "Copiar imagem com anotações (⌘C)",
        )));
        save.setToolTip(Some(&NSString::from_str("Salvar imagem em PNG… (⌘S)")));
        super::ui::primary(&copy);
        for b in [&copy, &save] {
            b.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
            header.addSubview(b);
        }

        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 42.0, 1040.0, 602.0));
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(true);
        scroll.setBackgroundColor(&super::ui::color(super::ui::Role::Surface));
        let clip: Retained<CenteredClipView> =
            unsafe { msg_send![CenteredClipView::alloc(mtm), initWithFrame: scroll.bounds()] };
        clip.setBackgroundColor(&super::ui::color(super::ui::Role::Surface));
        scroll.setContentView(&clip);
        scroll.setDocumentView(Some(&canvas));
        let board = super::ui::SurfaceView::create(mtm, rect(16.0, 56.0, 1008.0, 516.0));
        content.addSubview(&board);
        content.addSubview(&scroll);
        let minus = button("−", app, sel!(zoomOut:), rect(10.0, 8.0, 32.0, 27.0));
        let plus = button("+", app, sel!(zoomIn:), rect(43.0, 8.0, 32.0, 27.0));
        let fit_button = button("Ajustar", app, sel!(fitImage:), rect(81.0, 8.0, 75.0, 27.0));
        let actual = button("100%", app, sel!(actualSize:), rect(157.0, 8.0, 65.0, 27.0));
        minus.setAccessibilityLabel(Some(&NSString::from_str("Diminuir zoom")));
        plus.setAccessibilityLabel(Some(&NSString::from_str("Aumentar zoom")));
        undo.setToolTip(Some(&NSString::from_str("Desfazer (⌘Z)")));
        redo.setToolTip(Some(&NSString::from_str("Refazer (⌘⇧Z)")));
        for b in [&minus, &plus, &fit_button, &actual] {
            content.addSubview(b);
        }
        let metrics = label("", rect(236.0, 12.0, 220.0, 20.0), mtm);
        content.addSubview(&metrics);
        let status = label("", rect(460.0, 12.0, 564.0, 20.0), mtm);
        status.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&status);
        // Explicit tab order follows the visual groups even in the two-row layout.
        unsafe {
            tools.setNextKeyView(Some(&color));
            color.setNextKeyView(Some(&width));
            width.setNextKeyView(Some(&undo));
            undo.setNextKeyView(Some(&redo));
            redo.setNextKeyView(Some(&copy));
            copy.setNextKeyView(Some(&save));
            save.setNextKeyView(Some(&minus));
            minus.setNextKeyView(Some(&plus));
            plus.setNextKeyView(Some(&fit_button));
            fit_button.setNextKeyView(Some(&actual));
            actual.setNextKeyView(Some(&canvas));
            canvas.setNextKeyView(Some(&tools));
        }
        let weak_canvas = objc2::rc::Weak::new(&*canvas);
        let color_button = color.clone();
        let color_picker = ColorPicker::new(
            mtm,
            canvas.ivars().document.borrow().style.color,
            move |rgba| {
                if let Some(canvas) = weak_canvas.load() {
                    canvas.ivars().document.borrow_mut().style.color = rgba;
                    let image = super::color_picker::swatch(rgba);
                    image.setSize(NSSize::new(12.0, 12.0));
                    color_button.setImage(Some(&image));
                }
            },
        );
        color.setTitle(&NSString::from_str("Cor"));
        color.setImagePosition(NSCellImagePosition::ImageLeft);
        let image = super::color_picker::swatch(canvas.ivars().document.borrow().style.color);
        image.setSize(NSSize::new(12.0, 12.0));
        color.setImage(Some(&image));
        let editor = Self {
            window,
            canvas,
            scroll,
            status,
            undo,
            redo,
            color_picker,
            color_button: color,
            fit: Cell::new(true),
            toolbar,
            header,
            board,
            tools,
            width,
            copy,
            save,
            metrics,
            feedback: RefCell::new(String::new()),
        };
        editor.layout();
        editor.fit_image();
        editor.refresh();
        Ok(editor)
    }
    /// Lay out groups rather than shrinking controls when the window gets narrow.
    pub fn layout(&self) {
        let size = self.window.contentView().unwrap().bounds().size;
        let narrow = size.width < 960.0;
        let height = if narrow { 104.0 } else { 64.0 };
        let y = if narrow { 56.0 } else { 16.0 };
        self.toolbar
            .setFrame(rect(0.0, size.height - 64.0 - height, size.width, height));
        self.header
            .setFrame(rect(0.0, size.height - 64.0, size.width, 64.0));
        self.tools.setFrame(rect(super::ui::SPACE, y, 248.0, 36.0));
        for segment in 0..3 {
            self.tools.setWidth_forSegment(248.0 / 3.0, segment);
        }
        self.color_button.setFrame(rect(280.0, y, 80.0, 36.0));
        self.width.setFrame(rect(372.0, y, 88.0, 36.0));
        let history_y = if narrow { 12.0 } else { y };
        let history_x = if narrow { 16.0 } else { size.width - 196.0 };
        self.undo.setFrame(rect(history_x, history_y, 88.0, 36.0));
        self.redo
            .setFrame(rect(history_x + 96.0, history_y, 80.0, 36.0));
        self.copy
            .setFrame(rect(size.width - 280.0, 16.0, 120.0, 36.0));
        self.save
            .setFrame(rect(size.width - 148.0, 16.0, 124.0, 36.0));
        self.scroll.setFrame(rect(
            24.0,
            64.0,
            size.width - 48.0,
            size.height - height - 144.0,
        ));
        self.board.setFrame(rect(
            16.0,
            56.0,
            size.width - 32.0,
            size.height - height - 128.0,
        ));
        self.metrics.setFrame(rect(236.0, 14.0, 196.0, 20.0));
        self.status
            .setFrame(rect(440.0, 14.0, size.width - 456.0, 20.0));
    }
    pub fn export_feedback(&self, message: &str) {
        *self.feedback.borrow_mut() = message.into();
        self.refresh();
    }
    pub(super) fn color_picker_many_snapshot(&self) -> Option<Retained<NSBitmapImageRep>> {
        self.color_picker.snapshot_many_colors()
    }
    pub(super) fn color_picker_snapshot(&self) -> Option<Retained<NSBitmapImageRep>> {
        self.color_picker.snapshot()
    }
    pub(super) fn snapshots(&self, output: &std::path::Path) -> Result<(), String> {
        let original_size = self.window.contentView().unwrap().bounds().size;
        let original_appearance = self.window.appearance();
        for (theme, appearance) in [
            ("light", unsafe { NSAppearanceNameAqua }),
            ("dark", unsafe { NSAppearanceNameDarkAqua }),
        ] {
            self.window
                .setAppearance(NSAppearance::appearanceNamed(appearance).as_deref());
            for (name, size) in [
                ("standard", NSSize::new(1040.0, 700.0)),
                ("compact", NSSize::new(760.0, 450.0)),
            ] {
                self.window.setContentSize(size);
                self.layout();
                self.fit_image();
                self.window.displayIfNeeded();
                let content = self.window.contentView().unwrap();
                content.display();
                let bitmap = render::new_bitmap(size.width as usize, size.height as usize)?;
                content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
                let png = render::png(&bitmap)?;
                std::fs::write(output.join(format!("editor-{theme}-{name}.png")), unsafe {
                    png.as_bytes_unchecked()
                })
                .map_err(|e| e.to_string())?;
            }
            self.toggle_colors();
            if let Some(bitmap) = self.color_picker_snapshot() {
                let png = render::png(&bitmap)?;
                std::fs::write(output.join(format!("color-picker-{theme}.png")), unsafe {
                    png.as_bytes_unchecked()
                })
                .map_err(|e| e.to_string())?;
            }
            self.toggle_colors();
        }
        self.window.setAppearance(original_appearance.as_deref());
        self.window.setContentSize(original_size);
        self.layout();
        self.fit_image();
        Ok(())
    }
    pub(super) fn verify_editor_layout(&self) {
        // Dispatch through the actual custom controls, without touching the general clipboard.
        let original = self.canvas.ivars().document.borrow().annotations().count();
        if self.undo.isEnabled() {
            unsafe {
                self.undo.performClick(None);
            }
            assert!(self.redo.isEnabled());
            unsafe {
                self.redo.performClick(None);
            }
            assert_eq!(
                self.canvas.ivars().document.borrow().annotations().count(),
                original
            );
        }
        self.tools.setSelectedSegment(1);
        unsafe {
            assert!(
                self.tools
                    .sendAction_to(self.tools.action(), self.tools.target().as_deref())
            );
        }
        assert_eq!(self.canvas.ivars().document.borrow().tool, Tool::Rectangle);
        self.tools.setSelectedSegment(0);
        unsafe {
            assert!(
                self.tools
                    .sendAction_to(self.tools.action(), self.tools.target().as_deref())
            );
        }

        let assert_centered = || {
            let bounds = self.scroll.contentView().bounds();
            let image = self.canvas.frame();
            if image.size.width < bounds.size.width {
                assert!(
                    (bounds.origin.x + (bounds.size.width - image.size.width) / 2.0).abs() < 1.0
                );
            }
            if image.size.height < bounds.size.height {
                assert!(
                    (bounds.origin.y + (bounds.size.height - image.size.height) / 2.0).abs() < 1.0
                );
            }
        };
        let original_size = self.window.contentView().unwrap().bounds().size;
        for width in [760.0, 959.0, 960.0, 1040.0, 1440.0] {
            self.window.setContentSize(NSSize::new(width, 450.0));
            self.layout();
            self.fit_image();
            let expected = if width < 960.0 { 104.0 } else { 64.0 };
            assert_eq!(self.toolbar.frame().size.height, expected);
            let controls: [&NSView; 5] = [
                &self.tools,
                &self.color_button,
                &self.width,
                &self.undo,
                &self.redo,
            ];
            for (i, control) in controls.iter().enumerate() {
                let frame = control.frame();
                assert!(frame.origin.x >= 0.0 && frame.origin.x + frame.size.width <= width);
                for other in &controls[i + 1..] {
                    let other = other.frame();
                    assert!(
                        !(frame.origin.x < other.origin.x + other.size.width
                            && frame.origin.x + frame.size.width > other.origin.x
                            && frame.origin.y < other.origin.y + other.size.height
                            && frame.origin.y + frame.size.height > other.origin.y),
                        "toolbar controls overlap"
                    );
                }
            }
            assert_centered();
        }
        self.window.setContentSize(original_size);
        self.layout();
        self.fit_image();
        assert_centered();
        for zoom in [0.1, 0.25, 1.0] {
            self.set_zoom(zoom);
            assert_centered();
        }
        self.set_zoom(0.1);
        let original_frame = self.scroll.frame();
        let mut smaller = original_frame;
        smaller.size.width -= 130.0;
        smaller.size.height -= 80.0;
        self.scroll.setFrame(smaller);
        self.center_image();
        assert_centered();
        self.scroll.setFrame(original_frame);
        self.fit_image();
        self.toggle_colors();
        assert!(self.color_picker.is_open());
        assert_eq!(self.scroll.frame(), original_frame);
        assert_centered();
        let initial = self.canvas.ivars().document.borrow().style.color;
        self.color_picker.verify_change([0.25, 0.5, 0.75, 0.4]);
        assert_eq!(
            self.canvas.ivars().document.borrow().style.color,
            [0.25, 0.5, 0.75, 0.4]
        );
        self.color_picker.verify_change(initial);
        self.color_picker.verify_controls();
        self.toggle_colors();
        assert!(!self.color_picker.is_open());
        assert_eq!(self.scroll.frame(), original_frame);
        assert_centered();
        assert_eq!(
            NSApplication::sharedApplication(self.canvas.mtm()).activationPolicy(),
            NSApplicationActivationPolicy::Regular
        );
        assert!(!self.window.hidesOnDeactivate());
    }
    pub fn toggle_colors(&self) {
        if !self.color_picker.is_open() {
            self.color_picker
                .set_color(self.canvas.ivars().document.borrow().style.color);
        }
        self.color_picker.toggle(&self.color_button);
    }
    pub fn center_image(&self) {
        let clip = self.scroll.contentView();
        let bounds = clip.constrainBoundsRect(clip.bounds());
        clip.scrollToPoint(bounds.origin);
        self.scroll.reflectScrolledClipView(&clip);
    }
    pub fn show(&self) {
        NSApplication::sharedApplication(self.canvas.mtm())
            .setActivationPolicy(NSApplicationActivationPolicy::Regular);
        self.window.setHidesOnDeactivate(false);
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(Some(&self.canvas));
        super::activate(self.canvas.mtm());
    }
    pub fn fit_image(&self) {
        let size = self.scroll.contentSize();
        let image = &self.canvas.ivars().image;
        let zoom = ((size.width - 2.0) / image.width)
            .min((size.height - 2.0) / image.height)
            .clamp(0.01, 1.0);
        self.fit.set(true);
        self.canvas.set_zoom(zoom);
        self.canvas.scrollPoint(NSPoint::new(0.0, 0.0));
        self.center_image();
        self.refresh();
    }
    pub fn set_zoom(&self, zoom: f64) {
        self.fit.set(false);
        self.canvas.set_zoom(zoom.clamp(0.05, 4.0));
        self.center_image();
        self.refresh();
    }
    pub fn copy_to(&self, pasteboard: &NSPasteboard) -> Result<(), String> {
        let _operation = crate::telemetry::Operation::start("clipboard.copy");
        let data = self.canvas.export()?;
        pasteboard.clearContents();
        if !pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG }) {
            crate::telemetry::error("Clipboard", "Não foi possível copiar a imagem.");
            return Err("Não foi possível copiar a imagem.".into());
        }
        self.canvas.mark_exported();
        self.export_feedback("Imagem copiada");
        tracing::info!("clipboard.copied");
        Ok(())
    }
    pub fn refresh(&self) {
        let doc = self.canvas.ivars().document.borrow();
        self.undo.setEnabled(doc.can_undo());
        self.redo.setEnabled(doc.can_redo());
        if doc.needs_export() {
            self.feedback.borrow_mut().clear();
        }
        let feedback = self.feedback.borrow();
        let state = if doc.needs_export() {
            "Alterações não exportadas"
        } else if feedback.is_empty() {
            "Copiada ou salva"
        } else {
            &feedback
        };
        self.status.setStringValue(&NSString::from_str(state));
        self.status.setToolTip(Some(&NSString::from_str(state)));
        self.metrics.setStringValue(&NSString::from_str(&format!(
            "{} × {} px · {:.0}%",
            doc.width,
            doc.height,
            self.canvas.zoom() * 100.0
        )));
        self.window.setDocumentEdited(doc.needs_export());
    }
}
