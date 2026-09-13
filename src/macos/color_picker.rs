//! Reusable native dropdown. Owns its controls; communicates only through RGBA callbacks.
use super::render::rect;
use crate::color::{self, Rgba};
use objc2::{DefinedClass, MainThreadOnly, Message, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObjectProtocol, NSRect, NSRectEdge, NSSize, NSString,
    NSUserDefaults,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

fn native(c: Rgba) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c[0], c[1], c[2], c[3])
}
fn fill(frame: NSRect, c: Rgba) {
    native(c).setFill();
    NSBezierPath::bezierPathWithRect(frame).fill();
}
fn checker(frame: NSRect) {
    for y in 0..(frame.size.height / 5.0).ceil() as usize {
        for x in 0..(frame.size.width / 5.0).ceil() as usize {
            let v = if (x + y) % 2 == 0 { 0.55 } else { 0.8 };
            fill(
                rect(
                    frame.origin.x + x as f64 * 5.0,
                    frame.origin.y + y as f64 * 5.0,
                    5.0_f64.min(frame.size.width - x as f64 * 5.0),
                    5.0_f64.min(frame.size.height - y as f64 * 5.0),
                ),
                [v, v, v, 1.0],
            );
        }
    }
}

// Native button images keep AppKit's keyboard/focus behavior while showing exact sRGB swatches.
fn swatch(c: Rgba) -> Retained<NSImage> {
    let draw = block2::RcBlock::new(move |bounds: NSRect| {
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, 4.0, 4.0).addClip();
        checker(bounds);
        fill(bounds, c);
        NSGraphicsContext::restoreGraphicsState_class();
        objc2::runtime::Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(26.0, 26.0), false, &draw)
}

fn add_swatch_image() -> Retained<NSImage> {
    let draw = block2::RcBlock::new(|_: NSRect| {
        native([0.48, 0.48, 0.48, 1.0]).setStroke();
        let border = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            rect(0.5, 0.5, 25.0, 25.0),
            4.0,
            4.0,
        );
        border.setLineWidth(1.0);
        border.stroke();
        fill(rect(7.0, 12.25, 12.0, 1.5), [0.75, 0.75, 0.75, 1.0]);
        fill(rect(12.25, 7.0, 1.5, 12.0), [0.75, 0.75, 0.75, 1.0]);
        objc2::runtime::Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(26.0, 26.0), false, &draw)
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct PaletteView;
    unsafe impl NSObjectProtocol for PaletteView {}
    impl PaletteView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
    }
);

#[derive(Default)]
pub struct GradientIvars {
    stops: RefCell<Vec<Rgba>>,
}
define_class!(
    #[unsafe(super(NSSliderCell))]
    #[thread_kind = MainThreadOnly]
    #[ivars = GradientIvars]
    struct GradientCell;
    unsafe impl NSObjectProtocol for GradientCell {}
    impl GradientCell {
        #[unsafe(method(drawBarInside:flipped:))]
        fn draw_bar(&self, bounds: NSRect, _flipped: bool) {
            let bar = rect(bounds.origin.x, bounds.origin.y + (bounds.size.height-4.0)/2.0, bounds.size.width,4.0);
            checker(bar);
            let stops = self.ivars().stops.borrow();
            if stops.len() < 2 { return; }
            let width = bar.size.width.ceil() as usize;
            for x in 0..width {
                let p = x as f64 / (width-1).max(1) as f64 * (stops.len()-1) as f64;
                let i = (p.floor() as usize).min(stops.len()-2);
                let t = p-i as f64;
                let mut c = [0.0;4];
                for k in 0..4 { c[k] = stops[i][k]*(1.0-t)+stops[i+1][k]*t; }
                fill(rect(bar.origin.x+x as f64,bar.origin.y,1.0,bar.size.height),c);
            }
        }
        #[unsafe(method(drawKnob:))]
        fn draw_knob(&self, bounds: NSRect) {
            let knob = rect(bounds.origin.x+(bounds.size.width-12.0)/2.0,bounds.origin.y+(bounds.size.height-12.0)/2.0,12.0,12.0);
            NSColor::whiteColor().setFill();
            NSBezierPath::bezierPathWithOvalInRect(knob).fill();
            native([0.96,0.24,0.39,1.0]).setFill();
            NSBezierPath::bezierPathWithOvalInRect(rect(knob.origin.x+3.0,knob.origin.y+3.0,6.0,6.0)).fill();
        }
    }
);

#[derive(Default)]
pub struct PickerViewIvars {
    owner: RefCell<Weak<Inner>>,
    preview: Cell<Rgba>,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PickerViewIvars]
    struct PickerView;
    unsafe impl NSObjectProtocol for PickerView {}
    unsafe impl NSPopoverDelegate for PickerView {
        #[unsafe(method(popoverDidClose:))]
        fn did_close(&self, _notification: &NSNotification) {
            if NSApplication::sharedApplication(self.mtm()).currentEvent()
                .is_some_and(|event| event.r#type() == NSEventType::KeyDown && event.keyCode() == 53)
                && let Some(owner) = self.ivars().owner.borrow().upgrade()
                && let Some(anchor) = owner.anchor.borrow().as_ref()
                && let Some(window) = anchor.window() {
                    window.makeFirstResponder(Some(anchor));
                }
        }
    }
    impl PickerView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            fill(self.bounds(),[0.18,0.18,0.18,1.0]);
            let preview = rect(0.0,0.0,260.0,88.0);
            checker(preview);
            fill(preview,self.ivars().preview.get());
        }
        #[unsafe(method(cancelOperation:))]
        fn cancel(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                owner.popover.close();
                if let Some(anchor) = owner.anchor.borrow().as_ref()
                    && let Some(window) = anchor.window() { window.makeFirstResponder(Some(anchor)); }
            }
        }
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: &NSSlider) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() { owner.change_channel(sender.tag() as usize,sender.doubleValue()); }
        }
        #[unsafe(method(numberChanged:))]
        fn number_changed(&self, sender: &NSTextField) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                let index = sender.tag() as usize;
                match sender.stringValue().to_string().trim().parse::<f64>() {
                    Ok(value) if value.is_finite() && value >= 0.0 && value <= owner.maximum(index) => owner.change_channel(index,value),
                    _ => sender.setTextColor(Some(&NSColor::systemRedColor())),
                }
            }
        }
        #[unsafe(method(hexChanged:))]
        fn hex_changed(&self, sender: &NSTextField) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                if let Some(c) = color::parse_hex(&sender.stringValue().to_string(),owner.rgba.get()[3]) { owner.set_color(c,true); }
                else { sender.setTextColor(Some(&NSColor::systemRedColor())); }
            }
        }
        #[unsafe(method(modeChanged:))]
        fn mode_changed(&self, sender: &NSPopUpButton) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() { owner.rgb_mode.set(sender.indexOfSelectedItem()==1); owner.sync(); }
        }
        #[unsafe(method(copyColor:))]
        fn copy_color(&self, _sender: &NSButton) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                let c = owner.rgba.get();
                let pasteboard = NSPasteboard::generalPasteboard();
                pasteboard.clearContents();
                unsafe { pasteboard.setString_forType(&NSString::from_str(&format!("#{}",color::hex(c,c[3]<1.0))),NSPasteboardTypeString); }
            }
        }
        #[unsafe(method(addColor:))]
        fn add_color(&self, _sender: &NSButton) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                let encoded = color::hex(owner.rgba.get(),true);
                if !owner.palette.borrow().iter().any(|c| color::hex(*c,true)==encoded) {
                    owner.palette.borrow_mut().push(owner.rgba.get());
                    unsafe { NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&NSString::from_str(&color::encode_palette(&owner.palette.borrow()))),&NSString::from_str("colorPickerPalette")); }
                    owner.render_palette();
                    let index = owner.palette.borrow().len() - 1;
                    owner.palette_view.scrollRectToVisible(rect(
                        (index % 5) as f64 * 40.0, (index / 5) as f64 * 36.0, 32.0, 28.0));
                }
            }
        }
        #[unsafe(method(pickSaved:))]
        fn pick_saved(&self, sender: &NSButton) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                let c = owner.palette.borrow().get(sender.tag() as usize).copied();
                if let Some(c) = c { owner.set_color(c,true); }
            }
        }
        #[unsafe(method(sampleColor:))]
        fn sample_color(&self, _sender: &NSButton) {
            if let Some(owner) = self.ivars().owner.borrow().upgrade() {
                owner.popover.close();
                let weak = Rc::downgrade(&owner);
                let handler = block2::RcBlock::new(move |selected: *mut NSColor| {
                    let Some(owner) = weak.upgrade() else { return; };
                    if !owner.active.get() { return; }
                    if let Some(selected) = unsafe { selected.as_ref() }
                        && let Some(c) = selected.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) {
                            owner.set_color([c.redComponent(),c.greenComponent(),c.blueComponent(),owner.rgba.get()[3]],true);
                    }
                    owner.open();
                });
                unsafe { NSColorSampler::new().showSamplerWithSelectionHandler(&handler); }
            }
        }
    }
);

struct Inner {
    popover: Retained<NSPopover>,
    view: Retained<PickerView>,
    hex: Retained<NSTextField>,
    sliders: Vec<Retained<NSSlider>>,
    cells: Vec<Retained<GradientCell>>,
    numbers: Vec<Retained<NSTextField>>,
    labels: Vec<Retained<NSTextField>>,
    palette_view: Retained<NSView>,
    palette_scroll: Retained<NSScrollView>,
    palette: RefCell<Vec<Rgba>>,
    anchor: RefCell<Option<Retained<NSView>>>,
    rgba: Cell<Rgba>,
    hue: Cell<f64>,
    rgb_mode: Cell<bool>,
    active: Cell<bool>,
    changed: Box<dyn Fn(Rgba)>,
}

fn label(text: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    field.setTextColor(Some(&NSColor::lightGrayColor()));
    field
}
fn edit(
    text: &str,
    frame: NSRect,
    target: &PickerView,
    action: objc2::runtime::Sel,
) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(text), target.mtm());
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    field.setBezeled(false);
    field.setDrawsBackground(false);
    unsafe {
        field.setTarget(Some(target));
        field.setAction(Some(action));
        if let Some(cell) = field.cell() {
            let _: () = msg_send![&cell, setSendsActionOnEndEditing: true];
        }
    }
    field
}
fn button(
    title: &str,
    frame: NSRect,
    target: &PickerView,
    action: objc2::runtime::Sel,
    help: &str,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            target.mtm(),
        )
    };
    let symbol = match title {
        "⌾" => Some("eyedropper"),
        "⧉" => Some("square.on.square"),
        "+" => Some("plus"),
        _ => None,
    };
    if let Some(name) = symbol
        && let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(name),
            Some(&NSString::from_str(help)),
        )
    {
        button.setTitle(&NSString::from_str(""));
        button.setImage(Some(&image));
    }
    button.setFrame(frame);
    button.setBezelStyle(NSBezelStyle::Toolbar);
    button.setToolTip(Some(&NSString::from_str(help)));
    button.setAccessibilityLabel(Some(&NSString::from_str(help)));
    button
}

pub struct ColorPicker {
    inner: Rc<Inner>,
}
impl ColorPicker {
    pub fn new(mtm: MainThreadMarker, initial: Rgba, changed: impl Fn(Rgba) + 'static) -> Self {
        let view: Retained<PickerView> = unsafe {
            msg_send![super(PickerView::alloc(mtm).set_ivars(PickerViewIvars::default())), initWithFrame: rect(0.0,0.0,260.0,310.0)]
        };
        let hex = edit("", rect(16.0, 100.0, 92.0, 22.0), &view, sel!(hexChanged:));
        hex.setToolTip(Some(&NSString::from_str("Cor hexadecimal — seis dígitos")));
        hex.setAccessibilityLabel(Some(&NSString::from_str("Cor hexadecimal")));
        view.addSubview(&hex);
        let mode = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(117.0, 96.0, 64.0, 26.0),
            false,
        );
        for name in ["HSB", "RGB"] {
            mode.addItemWithTitle(&NSString::from_str(name));
        }
        unsafe {
            mode.setTarget(Some(&view));
            mode.setAction(Some(sel!(modeChanged:)));
        }
        mode.setAccessibilityLabel(Some(&NSString::from_str("Modelo de cor")));
        view.addSubview(&mode);
        view.addSubview(&button(
            "⌾",
            rect(185.0, 96.0, 28.0, 26.0),
            &view,
            sel!(sampleColor:),
            "Capturar cor da tela",
        ));
        view.addSubview(&button(
            "⧉",
            rect(217.0, 96.0, 28.0, 26.0),
            &view,
            sel!(copyColor:),
            "Copiar código HEX",
        ));
        let mut sliders = Vec::new();
        let mut cells = Vec::new();
        let mut numbers = Vec::new();
        let mut labels = Vec::new();
        for i in 0..4 {
            let y = 133.0 + i as f64 * 25.0;
            let caption = label("", rect(12.0, y + 4.0, 16.0, 16.0), mtm);
            view.addSubview(&caption);
            labels.push(caption);
            let slider = NSSlider::initWithFrame(NSSlider::alloc(mtm), rect(30.0, y, 177.0, 22.0));
            let cell: Retained<GradientCell> = unsafe {
                msg_send![
                    super(GradientCell::alloc(mtm).set_ivars(GradientIvars::default())),
                    init
                ]
            };
            slider.setCell(Some(&cell));
            slider.setMinValue(0.0);
            slider.setMaxValue(100.0);
            slider.setContinuous(true);
            slider.setTag(i);
            unsafe {
                slider.setTarget(Some(&view));
                slider.setAction(Some(sel!(sliderChanged:)));
            }
            view.addSubview(&slider);
            sliders.push(slider);
            cells.push(cell);
            let number = edit(
                "",
                rect(213.0, y + 2.0, 39.0, 20.0),
                &view,
                sel!(numberChanged:),
            );
            number.setTag(i);
            view.addSubview(&number);
            numbers.push(number);
        }
        view.addSubview(&label("Minha coleção", rect(16.0, 241.0, 170.0, 18.0), mtm));
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(12.0, 265.0, 204.0, 28.0));
        scroll.setDrawsBackground(false);
        scroll.setHasHorizontalScroller(false);
        scroll.setHasVerticalScroller(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setAutohidesScrollers(true);
        let palette_view: Retained<PaletteView> = unsafe {
            msg_send![super(PaletteView::alloc(mtm).set_ivars(())), initWithFrame: rect(0.0,0.0,204.0,28.0)]
        };
        let palette_view: Retained<NSView> = Retained::into_super(palette_view);
        scroll.setDocumentView(Some(&palette_view));
        view.addSubview(&scroll);
        let add = button(
            "+",
            rect(218.0, 265.0, 32.0, 28.0),
            &view,
            sel!(addColor:),
            "Salvar cor na coleção",
        );
        add.setBordered(false);
        add.setImage(Some(&add_swatch_image()));
        view.addSubview(&add);
        let controller = NSViewController::new(mtm);
        controller.setView(&view);
        let popover = NSPopover::new(mtm);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(NSSize::new(260.0, 310.0));
        popover.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*view)));
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAnimates(false);
        unsafe {
            popover
                .setAppearance(NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua).as_deref());
        }
        let saved = NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str("colorPickerPalette"));
        let palette = saved
            .map(|s| color::decode_palette(&s.to_string()))
            .unwrap_or_else(|| {
                color::decode_palette("F73E63FF,F76B2FFF,F7CD7FFF,49A7DBFF,12CD32FF")
            });
        let inner = Rc::new(Inner {
            popover,
            view,
            hex,
            sliders,
            cells,
            numbers,
            labels,
            palette_view,
            palette_scroll: scroll,
            palette: RefCell::new(palette),
            anchor: RefCell::new(None),
            rgba: Cell::new(initial),
            hue: Cell::new(0.0),
            rgb_mode: Cell::new(false),
            active: Cell::new(true),
            changed: Box::new(changed),
        });
        *inner.view.ivars().owner.borrow_mut() = Rc::downgrade(&inner);
        inner.set_color(initial, false);
        inner.render_palette();
        Self { inner }
    }
    pub fn set_color(&self, color: Rgba) {
        self.inner.set_color(color, false);
    }
    pub fn toggle(&self, anchor: &NSView) {
        if self.is_open() {
            self.close();
        } else {
            self.open(anchor);
        }
    }
    pub fn open(&self, anchor: &NSView) {
        *self.inner.anchor.borrow_mut() = Some(anchor.retain());
        self.inner.open();
    }
    pub fn close(&self) {
        self.inner.popover.close();
    }
    pub fn is_open(&self) -> bool {
        self.inner.popover.isShown()
    }
    pub fn invalidate(&self) {
        self.inner.active.set(false);
        self.close();
        self.inner.anchor.borrow_mut().take();
    }
    pub(super) fn snapshot_many_colors(&self) -> Option<Retained<NSBitmapImageRep>> {
        // Exercise overflow without changing the user's persistent collection.
        let examples = (0..23)
            .map(|i| color::rgb([i as f64 * 360.0 / 23.0, 0.75, 0.95], 1.0))
            .collect();
        let saved = self.inner.palette.replace(examples);
        self.inner.render_palette();
        let image = self.snapshot();
        self.inner.palette.replace(saved);
        self.inner.render_palette();
        image
    }
    pub(super) fn snapshot(&self) -> Option<Retained<NSBitmapImageRep>> {
        let bounds = self.inner.view.bounds();
        let bitmap = self
            .inner
            .view
            .bitmapImageRepForCachingDisplayInRect(bounds)?;
        self.inner
            .view
            .cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        Some(bitmap)
    }
    pub(super) fn verify_controls(&self) {
        let send = |control: &NSControl| {
            // Exercise the installed AppKit target/action with its actual sender type.
            unsafe {
                assert!(control.sendAction_to(control.action(), control.target().as_deref()));
            }
        };
        let inner = &self.inner;
        let initial = inner.rgba.get();
        inner.set_color([0.5, 0.5, 0.5, 0.4], false);
        inner.sliders[0].setDoubleValue(217.0);
        send(&inner.sliders[0]);
        assert!((inner.hue.get() - 217.0).abs() < 1e-10);
        inner.sliders[1].setDoubleValue(100.0);
        send(&inner.sliders[1]);
        assert!((color::hsv(inner.rgba.get(), 0.0)[0] - 217.0).abs() < 1e-10);
        inner.hex.setStringValue(&NSString::from_str("#F73E63"));
        send(&inner.hex);
        assert_eq!(inner.rgba.get(), color::parse_hex("F73E63", 0.4).unwrap());
        let before = inner.rgba.get();
        inner.hex.setStringValue(&NSString::from_str("invalid"));
        send(&inner.hex);
        assert_eq!(inner.rgba.get(), before);
        inner.rgb_mode.set(true);
        inner.sync();
        inner.numbers[1].setStringValue(&NSString::from_str("255"));
        send(&inner.numbers[1]);
        assert_eq!(inner.rgba.get()[1], 1.0);
        inner.numbers[1].setStringValue(&NSString::from_str("NaN"));
        send(&inner.numbers[1]);
        assert_eq!(inner.rgba.get()[1], 1.0);
        inner.sliders[3].setDoubleValue(25.0);
        send(&inner.sliders[3]);
        assert_eq!(inner.rgba.get()[3], 0.25);
        inner.rgb_mode.set(false);
        inner.set_color(initial, true);
    }
    pub(super) fn verify_change(&self, color: Rgba) {
        self.inner.set_color(color, true);
    }
}
impl Drop for ColorPicker {
    fn drop(&mut self) {
        self.invalidate();
    }
}

impl Inner {
    fn open(&self) {
        if !self.active.get() {
            return;
        }
        if let Some(anchor) = self.anchor.borrow().as_ref()
            && anchor.window().is_some_and(|w| w.isVisible())
        {
            self.popover.showRelativeToRect_ofView_preferredEdge(
                anchor.bounds(),
                anchor,
                NSRectEdge::MinY,
            );
        }
    }
    fn maximum(&self, i: usize) -> f64 {
        if i == 3 {
            100.0
        } else if self.rgb_mode.get() {
            255.0
        } else if i == 0 {
            360.0
        } else {
            100.0
        }
    }
    fn set_color(&self, c: Rgba, notify: bool) {
        if !c.iter().all(|v| v.is_finite()) {
            return;
        }
        let c = c.map(|v| v.clamp(0.0, 1.0));
        self.rgba.set(c);
        self.hue.set(color::hsv(c, self.hue.get())[0]);
        self.sync();
        if notify {
            (self.changed)(c);
        }
    }
    fn change_channel(&self, i: usize, value: f64) {
        let mut c = self.rgba.get();
        if i == 3 {
            c[3] = value / 100.0;
        } else if self.rgb_mode.get() {
            c[i] = value / 255.0;
        } else {
            let mut hsv = color::hsv(c, self.hue.get());
            hsv[i] = if i == 0 { value } else { value / 100.0 };
            self.hue.set(hsv[0]);
            c = color::rgb(hsv, c[3]);
        }
        self.set_color(c, true);
    }
    fn sync(&self) {
        let c = self.rgba.get();
        let hsv = color::hsv(c, self.hue.get());
        self.hex
            .setStringValue(&NSString::from_str(&color::hex(c, false)));
        self.hex.setTextColor(Some(&NSColor::whiteColor()));
        self.view.ivars().preview.set(c);
        self.view.setNeedsDisplay(true);
        let names = if self.rgb_mode.get() {
            ["Vermelho", "Verde", "Azul", "Opacidade"]
        } else {
            ["Matiz", "Saturação", "Brilho", "Opacidade"]
        };
        let short = if self.rgb_mode.get() {
            ["R", "G", "B", "A"]
        } else {
            ["H", "S", "B", "A"]
        };
        for i in 0..4 {
            let value = if i == 3 {
                c[3] * 100.0
            } else if self.rgb_mode.get() {
                c[i] * 255.0
            } else if i == 0 {
                hsv[0]
            } else {
                hsv[i] * 100.0
            };
            self.sliders[i].setAccessibilityLabel(Some(&NSString::from_str(names[i])));
            self.numbers[i]
                .setAccessibilityLabel(Some(&NSString::from_str(&format!("{} — valor", names[i]))));
            self.sliders[i].setMaxValue(self.maximum(i));
            self.sliders[i].setDoubleValue(value);
            self.sliders[i].setToolTip(Some(&NSString::from_str(names[i])));
            self.numbers[i].setToolTip(Some(&NSString::from_str(names[i])));
            self.numbers[i].setStringValue(&NSString::from_str(&format!("{value:.0}")));
            self.numbers[i].setTextColor(Some(&NSColor::lightGrayColor()));
            self.labels[i].setStringValue(&NSString::from_str(short[i]));
            let stops = (0..=6)
                .map(|step| {
                    let t = step as f64 / 6.0;
                    if i == 3 {
                        [c[0], c[1], c[2], t]
                    } else if self.rgb_mode.get() {
                        let mut stop = c;
                        stop[i] = t;
                        stop[3] = 1.0;
                        stop
                    } else {
                        let mut h = hsv;
                        if i == 0 {
                            h = [t * 360.0, 1.0, 1.0];
                        } else {
                            h[i] = t;
                        }
                        color::rgb(h, 1.0)
                    }
                })
                .collect();
            *self.cells[i].ivars().stops.borrow_mut() = stops;
            NSView::setNeedsDisplay(&self.sliders[i], true);
        }
    }
    fn render_palette(&self) {
        for child in self.palette_view.subviews() {
            child.removeFromSuperview();
        }
        let palette = self.palette.borrow();
        let rows = palette.len().div_ceil(5).max(1);
        let content_height = rows as f64 * 36.0 - 8.0;
        let visible_height = rows.min(3) as f64 * 36.0 - 8.0;
        self.palette_view
            .setFrameSize(NSSize::new(204.0, content_height));
        self.palette_scroll
            .setFrameSize(NSSize::new(204.0, visible_height));
        self.popover
            .setContentSize(NSSize::new(260.0, 265.0 + visible_height + 14.0));
        // Re-clamp after a shorter collection is restored by diagnostics.
        let clip = self.palette_scroll.contentView();
        let bounds = clip.constrainBoundsRect(clip.bounds());
        clip.scrollToPoint(bounds.origin);
        self.palette_scroll.reflectScrolledClipView(&clip);
        for (i, c) in palette.iter().enumerate() {
            let button = button(
                "",
                rect((i % 5) as f64 * 40.0, (i / 5) as f64 * 36.0, 32.0, 28.0),
                &self.view,
                sel!(pickSaved:),
                &format!("Selecionar #{}", color::hex(*c, true)),
            );
            button.setTag(i as isize);
            button.setBordered(false);
            button.setImage(Some(&swatch(*c)));
            self.palette_view.addSubview(&button);
        }
    }
}
