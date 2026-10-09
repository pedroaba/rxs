//! Shared native presentation. Views inherit the application appearance, including popovers.
use objc2::sel;
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSRect, NSString, NSUserDefaults};
use std::{cell::Cell, ptr::NonNull};

/// Roles shared by every custom component; providers resolve against the owning view.
#[derive(Clone, Copy)]
pub enum Role {
    Background,
    Surface,
    Raised,
    Border,
    Text,
    Muted,
    Accent,
    Hover,
    Selected,
    Danger,
    Link,
    Warning,
}
fn values(role: Role) -> (u32, u32) {
    match role {
        Role::Background => (0x101115, 0xf3f4f7),
        Role::Surface => (0x181a20, 0xffffff),
        Role::Raised => (0x20232b, 0xf8f9fb),
        Role::Border => (0x343844, 0xd4d9e2),
        Role::Text => (0xf0f2f7, 0x191e2b),
        Role::Muted => (0xa7aebf, 0x586174),
        Role::Accent => (0x3c6be0, 0x2857cd),
        Role::Hover => (0x2b303b, 0xe9edf4),
        Role::Selected => (0x26365b, 0xe8eeff),
        Role::Danger => (0xf98f92, 0xb42333),
        Role::Link => (0x8aafff, 0x2451bf),
        Role::Warning => (0xf0be73, 0x8a4b0a),
    }
}
pub fn color(role: Role) -> Retained<NSColor> {
    let (dark, light) = values(role);
    let rgb = |value: u32| {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            ((value >> 16) & 255) as f64 / 255.0,
            ((value >> 8) & 255) as f64 / 255.0,
            (value & 255) as f64 / 255.0,
            1.0,
        )
    };
    let dark = rgb(dark);
    let light = rgb(light);
    let provider = block2::RcBlock::new(move |appearance: NonNull<NSAppearance>| {
        // AppKit supplies a live appearance and retains this block with its captured colors.
        if unsafe { appearance.as_ref() }
            .name()
            .to_string()
            .contains("Dark")
        {
            NonNull::from(&*dark)
        } else {
            NonNull::from(&*light)
        }
    });
    unsafe { NSColor::colorWithName_dynamicProvider(None, &provider) }
}
pub fn panel(frame: NSRect, radius: f64, fill: Role, border: bool) {
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, radius, radius);
    color(fill).setFill();
    path.fill();
    if border {
        color(Role::Border).setStroke();
        path.setLineWidth(1.0);
        path.stroke();
    }
}
pub fn text(view: &NSView, value: &str, frame: NSRect, color: &NSColor, align: NSTextAlignment) {
    let cell = NSTextFieldCell::initTextCell(
        NSTextFieldCell::alloc(view.mtm()),
        &NSString::from_str(value),
    );
    cell.setFont(Some(&NSFont::systemFontOfSize(BODY)));
    cell.setTextColor(Some(color));
    cell.setAlignment(align);
    cell.drawWithFrame_inView(frame, view);
}
fn inset(frame: NSRect, amount: f64) -> NSRect {
    super::render::rect(
        frame.origin.x + amount,
        frame.origin.y + amount,
        (frame.size.width - amount * 2.0).max(0.0),
        (frame.size.height - amount * 2.0).max(0.0),
    )
}
fn focused(view: &NSView) -> bool {
    view.window()
        .and_then(|window| window.firstResponder())
        .is_some_and(|responder| std::ptr::eq::<NSResponder>(&*responder, &**view))
}
fn focus_border(view: &NSView) {
    if focused(view) {
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            inset(view.bounds(), 1.5),
            7.0,
            7.0,
        );
        color(Role::Accent).setStroke();
        path.setLineWidth(2.0);
        path.stroke();
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct ChromeView;
    unsafe impl NSObjectProtocol for ChromeView {}
    impl ChromeView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            color(Role::Background).setFill(); NSBezierPath::bezierPathWithRect(self.bounds()).fill();
            color(Role::Border).setFill();
            NSBezierPath::bezierPathWithRect(super::render::rect(0.0, 0.0, self.bounds().size.width, 1.0)).fill();
        }
    }
);
impl ChromeView {
    pub fn create(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSView> {
        let view: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] };
        view.into_super()
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct SurfaceView;
    unsafe impl NSObjectProtocol for SurfaceView {}
    impl SurfaceView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) { panel(inset(self.bounds(), 0.5), 12.0, Role::Surface, true); }
    }
);
impl SurfaceView {
    pub fn create(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSView> {
        let view: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] };
        view.into_super()
    }
}

#[derive(Default)]
pub struct ButtonIvars {
    hovered: Cell<bool>,
}
// AppKit still owns action dispatch, key equivalents, tracking, and accessibility.
define_class!(
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ButtonIvars]
    struct WebButton;
    unsafe impl NSObjectProtocol for WebButton {}
    impl WebButton {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let primary = self.tag() == PRIMARY_TAG;
            let enabled = self.isEnabled();
            let fill = if primary && enabled { Role::Accent }
                else if self.isHighlighted() || (self.ivars().hovered.get() && enabled) { Role::Hover }
                else { Role::Raised };
            panel(inset(self.bounds(), 0.5), 7.0, fill, !primary);
            let foreground = if !enabled { color(Role::Muted).colorWithAlphaComponent(0.45) }
                else if primary { NSColor::whiteColor() } else { color(Role::Text) };
            let bounds = self.bounds();
            let title = self.title().to_string();
            let mut title_frame = super::render::rect(8.0, (bounds.size.height - 19.0) / 2.0, bounds.size.width - 16.0, 19.0);
            if let Some(image) = self.image() {
                let size = if title.is_empty() { 18.0 } else { 12.0 };
                let x = if title.is_empty() { (bounds.size.width - size) / 2.0 } else { 12.0 };
                let frame = super::render::rect(x, (bounds.size.height-size)/2.0, size, size);
                if let Some(cell) = self.cell() {
                    let cell: &NSButtonCell = unsafe { &*(std::ptr::from_ref(&*cell).cast()) };
                    cell.drawImage_withFrame_inView(&image, frame, self);
                }
                if !title.is_empty() { title_frame.origin.x = 30.0; title_frame.size.width = bounds.size.width - 38.0; }
            }
            text(self, &title, title_frame, &foreground, NSTextAlignment::Center);
            if self.isHighlighted() && enabled {
                NSColor::colorWithWhite_alpha(0.0, 0.12).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inset(bounds, 0.5), 7.0, 7.0).fill();
            }
            focus_border(self);
        }
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking(&self) {
            for area in self.trackingAreas() { self.removeTrackingArea(&area); }
            let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), self.bounds(),
                NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveInKeyWindow | NSTrackingAreaOptions::InVisibleRect,
                Some(self), None) };
            self.addTrackingArea(&area);
        }
        #[unsafe(method(mouseEntered:))]
        fn entered(&self, _event: &NSEvent) { self.ivars().hovered.set(true); NSView::setNeedsDisplay(self, true); }
        #[unsafe(method(mouseExited:))]
        fn exited(&self, _event: &NSEvent) { self.ivars().hovered.set(false); NSView::setNeedsDisplay(self, true); }
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) { NSView::setNeedsDisplay(self, true); }
    }
);
pub fn button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let button: Retained<WebButton> = unsafe {
        msg_send![super(WebButton::alloc(mtm).set_ivars(ButtonIvars::default())), initWithFrame: frame]
    };
    button.setTitle(&NSString::from_str(title));
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(action));
    }
    button.setButtonType(NSButtonType::MomentaryPushIn);
    button.setBordered(false);
    button.setFocusRingType(NSFocusRingType::None);
    style_button(&button);
    button.into_super()
}

define_class!(
    #[unsafe(super(NSPopUpButton))]
    #[thread_kind = MainThreadOnly]
    struct WebSelect;
    unsafe impl NSObjectProtocol for WebSelect {}
    impl WebSelect {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            panel(inset(self.bounds(), 0.5), 7.0, Role::Raised, true);
            let bounds = self.bounds();
            text(self, &self.title().to_string(), super::render::rect(10.0, (bounds.size.height-19.0)/2.0, bounds.size.width-32.0, 19.0), &color(Role::Text), NSTextAlignment::Left);
            text(self, "⌄", super::render::rect(bounds.size.width-24.0, (bounds.size.height-19.0)/2.0, 16.0, 19.0), &color(Role::Muted), NSTextAlignment::Center);
            focus_border(self);
        }
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) { NSView::setNeedsDisplay(self, true); }
    }
);
pub fn select(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSPopUpButton> {
    let select: Retained<WebSelect> =
        unsafe { msg_send![WebSelect::alloc(mtm), initWithFrame: frame, pullsDown: false] };
    select.setBordered(false);
    select.setFocusRingType(NSFocusRingType::None);
    select.into_super()
}

define_class!(
    #[unsafe(super(NSSegmentedControl))]
    #[thread_kind = MainThreadOnly]
    struct WebSegments;
    unsafe impl NSObjectProtocol for WebSegments {}
    impl WebSegments {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let bounds = self.bounds(); let count = self.segmentCount();
            panel(inset(bounds, 0.5), 8.0, Role::Surface, true);
            let width = bounds.size.width / count.max(1) as f64;
            for i in 0..count {
                let selected = self.selectedSegment() == i;
                if selected { panel(super::render::rect(i as f64 * width + 3.0, 3.0, width-6.0, bounds.size.height-6.0), 5.0, Role::Selected, false); }
                let title = self.labelForSegment(i).map(|label| label.to_string()).unwrap_or_default();
                text(self, &title, super::render::rect(i as f64 * width + 4.0, (bounds.size.height-19.0)/2.0, width-8.0, 19.0), &color(if selected { Role::Text } else { Role::Muted }), NSTextAlignment::Center);
            }
            focus_border(self);
        }
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) { NSView::setNeedsDisplay(self, true); }
    }
);
pub fn segments(
    mtm: MainThreadMarker,
    titles: &[&str],
    target: &AnyObject,
    action: Sel,
    frame: NSRect,
) -> Retained<NSSegmentedControl> {
    let control: Retained<WebSegments> =
        unsafe { msg_send![WebSegments::alloc(mtm), initWithFrame: frame] };
    control.setSegmentCount(titles.len() as isize);
    for (i, title) in titles.iter().enumerate() {
        control.setLabel_forSegment(&NSString::from_str(title), i as isize);
        control.setWidth_forSegment(frame.size.width / titles.len() as f64, i as isize);
    }
    control.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(action));
    }
    control.setFocusRingType(NSFocusRingType::None);
    control.into_super()
}

#[derive(Default)]
struct TableIvars {
    hovered: Cell<Option<isize>>,
}
define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TableIvars]
    struct WebTable;
    unsafe impl NSObjectProtocol for WebTable {}
    impl WebTable {
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            let accepted: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            NSView::setNeedsDisplay(self, true); accepted
        }
        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            let accepted: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            NSView::setNeedsDisplay(self, true); accepted
        }
        #[unsafe(method(drawBackgroundInClipRect:))]
        fn background(&self, clip: NSRect) {
            color(Role::Surface).setFill(); NSBezierPath::bezierPathWithRect(clip).fill();
            let range = self.rowsInRect(clip);
            for row in range.location..range.location + range.length {
                let frame = self.rectOfRow(row as isize);
                color(Role::Border).colorWithAlphaComponent(0.4).setFill();
                NSBezierPath::bezierPathWithRect(super::render::rect(frame.origin.x+12.0, frame.origin.y+frame.size.height-0.5, frame.size.width-24.0, 0.5)).fill();
            }
            if let Some(row) = self.ivars().hovered.get().filter(|row| *row >= 0 && *row < self.numberOfRows() && *row != self.selectedRow()) {
                panel(inset(self.rectOfRow(row), 4.0), 6.0, Role::Raised, false);
            }
        }
        #[unsafe(method(highlightSelectionInClipRect:))]
        fn highlight_selection(&self, _clip: NSRect) {
            let row = self.selectedRow();
            if row >= 0 {
                let frame = inset(self.rectOfRow(row), 4.0);
                panel(frame, 6.0, Role::Selected, false);
                if focused(self) {
                    color(Role::Link).setFill();
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(super::render::rect(frame.origin.x, frame.origin.y+8.0, 3.0, frame.size.height-16.0), 1.5, 1.5).fill();
                }
            }
        }
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking(&self) {
            unsafe { let _: () = msg_send![super(self), updateTrackingAreas]; }
            for area in self.trackingAreas() { self.removeTrackingArea(&area); }
            let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), self.bounds(),
                NSTrackingAreaOptions::MouseMoved | NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveInKeyWindow | NSTrackingAreaOptions::InVisibleRect,
                Some(self), None) };
            self.addTrackingArea(&area);
        }
        #[unsafe(method(mouseMoved:))]
        fn moved(&self, event: &NSEvent) {
            let row = self.rowAtPoint(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().hovered.set(Some(row)); NSView::setNeedsDisplay(self, true);
        }
        #[unsafe(method(mouseExited:))]
        fn exited(&self, _event: &NSEvent) { self.ivars().hovered.set(None); NSView::setNeedsDisplay(self, true); }
    }
);
pub fn table(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSTableView> {
    let table: Retained<WebTable> = unsafe {
        msg_send![super(WebTable::alloc(mtm).set_ivars(TableIvars::default())), initWithFrame: frame]
    };
    table.setFocusRingType(NSFocusRingType::None);
    table.into_super()
}

/// Clip scrolling surfaces without replacing native scrolling or hit testing.
pub fn round_scroll(scroll: &NSScrollView) {
    scroll.setWantsLayer(true);
    let layer: Option<Retained<AnyObject>> = unsafe { msg_send![scroll, layer] };
    if let Some(layer) = layer {
        unsafe {
            let _: () = msg_send![&*layer, setCornerRadius: 10.0f64];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}

define_class!(
    #[unsafe(super(NSSplitView))]
    #[thread_kind = MainThreadOnly]
    struct WebSplit;
    unsafe impl NSObjectProtocol for WebSplit {}
    impl WebSplit {
        #[unsafe(method(dividerThickness))]
        fn thickness(&self) -> f64 { 12.0 }
        #[unsafe(method(drawDividerInRect:))]
        fn divider(&self, frame: NSRect) {
            color(Role::Background).setFill(); NSBezierPath::bezierPathWithRect(frame).fill();
            color(Role::Border).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(super::render::rect(frame.origin.x+5.0, frame.origin.y+frame.size.height/2.0-16.0, 2.0, 32.0), 1.0, 1.0).fill();
        }
    }
);
pub fn split(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSSplitView> {
    let view: Retained<WebSplit> = unsafe { msg_send![WebSplit::alloc(mtm), initWithFrame: frame] };
    view.into_super()
}

// Cell based tables keep their native keyboard and accessibility semantics while
// owning the rendering of typography, padding and semantic status badges.
define_class!(
    #[unsafe(super(NSTextFieldCell))]
    #[thread_kind = MainThreadOnly]
    struct WebTableCell;
    unsafe impl NSObjectProtocol for WebTableCell {}
    impl WebTableCell {
        #[unsafe(method(drawWithFrame:inView:))]
        fn draw(&self, frame: NSRect, view: &NSView) {
            let column = self.representedObject().map(|value| unsafe { msg_send![&*value, description] }).unwrap_or_else(|| NSString::from_str("message"));
            let value = self.stringValue().to_string();
            let content = super::render::rect(frame.origin.x+12.0, frame.origin.y+(frame.size.height-18.0)/2.0, frame.size.width-24.0, 18.0);
            if column.to_string() == "kind" {
                let role = match value.as_str() { "Erro" => Role::Danger, "Operação" => Role::Link,
                    "Aviso" => Role::Warning, _ => Role::Muted };
                let width = match value.as_str() { "Informação" => 94.0, "Operação" => 84.0, _ => 66.0 };
                let badge = super::render::rect(content.origin.x, frame.origin.y+(frame.size.height-24.0)/2.0, width, 24.0);
                let tint = color(role);
                tint.colorWithAlphaComponent(0.08).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(badge, 5.0, 5.0).fill();
                tint.setFill();
                NSBezierPath::bezierPathWithOvalInRect(super::render::rect(badge.origin.x+8.0, badge.origin.y+9.0, 6.0, 6.0)).fill();
                text(view, &value, super::render::rect(badge.origin.x+19.0, content.origin.y, width-23.0, 18.0), &tint, NSTextAlignment::Left);
            } else {
                let cell = NSTextFieldCell::initTextCell(NSTextFieldCell::alloc(view.mtm()), &NSString::from_str(&value));
                let time = column.to_string() == "time";
                let font = if time { NSFont::monospacedDigitSystemFontOfSize_weight(12.0,0.0) } else { NSFont::systemFontOfSize(13.0) };
                cell.setFont(Some(&font));
                cell.setTextColor(Some(&color(if time { Role::Muted } else { Role::Text })));
                cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                cell.drawWithFrame_inView(content, view);
            }
        }
    }
);
pub fn table_cell(mtm: MainThreadMarker, column: &str) -> Retained<NSTextFieldCell> {
    let cell: Retained<WebTableCell> =
        unsafe { msg_send![WebTableCell::alloc(mtm), initTextCell: &*NSString::from_str("")] };
    unsafe {
        cell.setRepresentedObject(Some(&NSString::from_str(column)));
    }
    cell.into_super()
}

define_class!(
    #[unsafe(super(NSTableHeaderView))]
    #[thread_kind = MainThreadOnly]
    struct WebTableHeader;
    unsafe impl NSObjectProtocol for WebTableHeader {}
    impl WebTableHeader {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            color(Role::Raised).setFill(); NSBezierPath::bezierPathWithRect(self.bounds()).fill();
            if let Some(table) = self.tableView() {
                for (index, column) in table.tableColumns().iter().enumerate() {
                    let mut frame = self.headerRectOfColumn(index as isize);
                    frame.origin.x += 10.0; frame.size.width -= 20.0;
                    frame.origin.y = (frame.size.height-19.0)/2.0; frame.size.height = 19.0;
                    text(self, &column.title().to_string(), frame, &color(Role::Muted), NSTextAlignment::Left);
                }
            }
        }
    }
);
pub fn table_header(mtm: MainThreadMarker) -> Retained<NSTableHeaderView> {
    let view: Retained<WebTableHeader> = unsafe {
        msg_send![WebTableHeader::alloc(mtm), initWithFrame: super::render::rect(0.0,0.0,432.0,36.0)]
    };
    view.into_super()
}

define_class!(
    #[unsafe(super(NSTextFieldCell))]
    #[thread_kind = MainThreadOnly]
    struct WebInputCell;
    unsafe impl NSObjectProtocol for WebInputCell {}
    impl WebInputCell {
        #[unsafe(method(selectWithFrame:inView:editor:delegate:start:length:))]
        unsafe fn select_text(&self, bounds: NSRect, view: &NSView, editor: &NSText, delegate: Option<&AnyObject>, start: isize, length: isize) {
            let frame = super::render::rect(bounds.origin.x+8.0, bounds.origin.y+(bounds.size.height-18.0)/2.0, bounds.size.width-16.0, 18.0);
            unsafe { let _: () = msg_send![super(self), selectWithFrame: frame, inView: view, editor: editor, delegate: delegate, start: start, length: length]; }
        }
        #[unsafe(method(editWithFrame:inView:editor:delegate:event:))]
        unsafe fn edit_text(&self, bounds: NSRect, view: &NSView, editor: &NSText, delegate: Option<&AnyObject>, event: Option<&NSEvent>) {
            let frame = super::render::rect(bounds.origin.x+8.0, bounds.origin.y+(bounds.size.height-18.0)/2.0, bounds.size.width-16.0, 18.0);
            unsafe { let _: () = msg_send![super(self), editWithFrame: frame, inView: view, editor: editor, delegate: delegate, event: event]; }
        }
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect(&self, bounds: NSRect) -> NSRect {
            let height = self.cellSize().height.min(bounds.size.height);
            super::render::rect(bounds.origin.x+8.0, bounds.origin.y+(bounds.size.height-height)/2.0, bounds.size.width-16.0, height)
        }
    }
);
// Text editing remains native; custom chrome replaces the system field bezel.
define_class!(
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    struct WebInput;
    unsafe impl NSObjectProtocol for WebInput {}
    impl WebInput {
        #[unsafe(method(drawRect:))]
        fn draw(&self, dirty: NSRect) {
            panel(inset(self.bounds(), 0.5), 6.0, Role::Surface, true);
            unsafe { let _: () = msg_send![super(self), drawRect: dirty]; }
            if self.currentEditor().is_some() {
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inset(self.bounds(), 1.0), 6.0, 6.0);
                color(Role::Accent).setStroke(); path.setLineWidth(2.0); path.stroke();
            }
        }
    }
);
pub fn input(mtm: MainThreadMarker, value: &str, frame: NSRect) -> Retained<NSTextField> {
    let input: Retained<WebInput> =
        unsafe { msg_send![WebInput::alloc(mtm), initWithFrame: frame] };
    let cell: Retained<WebInputCell> =
        unsafe { msg_send![WebInputCell::alloc(mtm), initTextCell: &*NSString::from_str(value)] };
    cell.setScrollable(true);
    cell.setEditable(true);
    cell.setSelectable(true);
    input.setCell(Some(&cell));
    input.setStringValue(&NSString::from_str(value));
    input.setBezeled(false);
    input.setDrawsBackground(false);
    input.setFont(Some(&NSFont::systemFontOfSize(BODY)));
    input.setTextColor(Some(&color(Role::Text)));
    input.into_super()
}

pub struct AppearanceIvars {
    original: Theme,
    control: std::cell::RefCell<Option<Retained<NSSegmentedControl>>>,
}
define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppearanceIvars]
    struct AppearanceWindow;
    unsafe impl NSObjectProtocol for AppearanceWindow {}
    unsafe impl NSWindowDelegate for AppearanceWindow {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _window: &NSWindow) -> bool { self.cancel(sel!(cancelTheme:), None); false }
    }
    impl AppearanceWindow {
        #[unsafe(method(previewTheme:))]
        fn preview(&self, control: &NSSegmentedControl) { selected_theme(control.selectedSegment()).apply(self.mtm()); }
        #[unsafe(method(cancelTheme:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            self.ivars().original.apply(self.mtm());
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
        #[unsafe(method(applyTheme:))]
        fn apply(&self, _sender: Option<&AnyObject>) {
            let index = self.ivars().control.borrow().as_ref().unwrap().selectedSegment();
            selected_theme(index).save(self.mtm());
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }
);
fn selected_theme(index: isize) -> Theme {
    match index {
        1 => Theme::Light,
        2 => Theme::System,
        _ => Theme::Dark,
    }
}
fn appearance_window(mtm: MainThreadMarker) -> Retained<AppearanceWindow> {
    let original = Theme::load();
    let window: Retained<AppearanceWindow> = unsafe {
        msg_send![super(AppearanceWindow::alloc(mtm).set_ivars(AppearanceIvars { original, control: std::cell::RefCell::new(None) })),
        initWithContentRect: super::render::rect(0.0,0.0,560.0,264.0),
        styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
        backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str("RSX — Aparência"));
    window.setDelegate(Some(ProtocolObject::from_ref(&*window)));
    let content = ChromeView::create(mtm, super::render::rect(0.0, 0.0, 560.0, 264.0));
    window.setContentView(Some(&content));
    let title = super::editor::label(
        "Aparência",
        super::render::rect(28.0, 207.0, 504.0, 28.0),
        mtm,
    );
    title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
    title.setTextColor(Some(&color(Role::Text)));
    content.addSubview(&title);
    content.addSubview(&super::editor::label(
        "Escolha o tema do seu ambiente de trabalho.",
        super::render::rect(28.0, 174.0, 504.0, 22.0),
        mtm,
    ));
    let control = segments(
        mtm,
        &["Escuro", "Claro", "Sistema"],
        &window,
        sel!(previewTheme:),
        super::render::rect(28.0, 112.0, 504.0, 44.0),
    );
    control.setSelectedSegment(match original {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::System => 2,
    });
    control.setAccessibilityLabel(Some(&NSString::from_str("Tema da interface")));
    content.addSubview(&control);
    content.addSubview(&super::editor::label(
        "Sistema acompanha a aparência definida no macOS.",
        super::render::rect(28.0, 78.0, 504.0, 22.0),
        mtm,
    ));
    let cancel = button(
        mtm,
        "Cancelar",
        &window,
        sel!(cancelTheme:),
        super::render::rect(304.0, 24.0, 104.0, 36.0),
    );
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);
    let apply = button(
        mtm,
        "Aplicar",
        &window,
        sel!(applyTheme:),
        super::render::rect(420.0, 24.0, 112.0, 36.0),
    );
    primary(&apply);
    apply.setKeyEquivalent(&NSString::from_str("\r"));
    content.addSubview(&apply);
    window.setInitialFirstResponder(Some(&control));
    *window.ivars().control.borrow_mut() = Some(control);
    window.center();
    window
}
pub fn show_appearance(mtm: MainThreadMarker) {
    let window = appearance_window(mtm);
    super::activate(mtm);
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    window.close();
}

/// Exercises inherited appearance without changing the saved user preference.
pub fn verify(mtm: MainThreadMarker, output: &std::path::Path) {
    let app = NSApplication::sharedApplication(mtm);
    let original = app.appearance();
    let window = unsafe { NSWindow::new(mtm) };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    for (theme, expected) in [
        (Theme::Dark, unsafe { NSAppearanceNameDarkAqua }),
        (Theme::Light, unsafe { NSAppearanceNameAqua }),
    ] {
        theme.apply(mtm);
        assert_eq!(&*window.effectiveAppearance().name(), expected);
        assert_eq!(
            &*window.contentView().unwrap().effectiveAppearance().name(),
            expected
        );
    }
    Theme::System.apply(mtm);
    assert!(app.appearance().is_none());
    app.setAppearance(original.as_deref());
    window.close();
    let chooser = appearance_window(mtm);
    let control = chooser.ivars().control.borrow().as_ref().unwrap().clone();
    for (name, index) in [("dark", 0), ("light", 1)] {
        control.setSelectedSegment(index);
        chooser.preview(sel!(previewTheme:), &control);
        chooser.orderFront(None);
        chooser.displayIfNeeded();
        let content = chooser.contentView().unwrap();
        content.display();
        let bitmap = super::render::new_bitmap(560, 264).unwrap();
        content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
        let png = super::render::png(&bitmap).unwrap();
        std::fs::write(output.join(format!("appearance-{name}.png")), unsafe {
            png.as_bytes_unchecked()
        })
        .unwrap();
    }
    chooser.cancel(sel!(cancelTheme:), None);
    match chooser.ivars().original {
        Theme::System => assert!(app.appearance().is_none()),
        Theme::Dark => assert_eq!(&*app.effectiveAppearance().name(), unsafe {
            NSAppearanceNameDarkAqua
        }),
        Theme::Light => assert_eq!(&*app.effectiveAppearance().name(), unsafe {
            NSAppearanceNameAqua
        }),
    }
    chooser.setDelegate(None);
    chooser.close();
    app.setAppearance(original.as_deref());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    System,
}
impl Theme {
    pub fn parse(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "system" => Self::System,
            _ => Self::Dark,
        }
    }
    fn value(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }
    pub fn load() -> Self {
        NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str("appearance"))
            .map(|value| Self::parse(&value.to_string()))
            .unwrap_or(Self::Dark)
    }
    pub fn apply(self, mtm: MainThreadMarker) {
        let appearance = match self {
            Self::Dark => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
            Self::Light => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
            Self::System => None,
        };
        NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
    }
    pub fn save(self, mtm: MainThreadMarker) {
        unsafe {
            NSUserDefaults::standardUserDefaults().setObject_forKey(
                Some(&NSString::from_str(self.value())),
                &NSString::from_str("appearance"),
            );
        }
        self.apply(mtm);
    }
}
const PRIMARY_TAG: isize = -100;
pub const BODY: f64 = 13.0;

pub const SPACE: f64 = 16.0;
pub fn style_button(button: &NSButton) {
    button.setFont(Some(&NSFont::systemFontOfSize(BODY)));
    button.setBordered(false);
    button.setContentTintColor(Some(&color(Role::Text)));
    button.setAccessibilityLabel(Some(&button.title()));
}
pub fn primary(button: &NSButton) {
    style_button(button);
    button.setTag(PRIMARY_TAG);
    button.setContentTintColor(Some(&NSColor::whiteColor()));
    NSView::setNeedsDisplay(button, true);
}
pub fn secondary_label(label: &NSTextField) {
    label.setFont(Some(&NSFont::systemFontOfSize(BODY)));
    label.setTextColor(Some(&color(Role::Muted)));
}
#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(rgb: u32) -> f64 {
        let channel = |shift| {
            let value = ((rgb >> shift) & 255u32) as f64 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        channel(16) * 0.2126 + channel(8) * 0.7152 + channel(0) * 0.0722
    }
    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn theme_tokens_keep_text_and_focus_readable() {
        for light in [false, true] {
            let value = |role| {
                let pair = values(role);
                if light { pair.1 } else { pair.0 }
            };
            for background in [
                Role::Background,
                Role::Surface,
                Role::Raised,
                Role::Selected,
                Role::Hover,
            ] {
                for foreground in [Role::Text, Role::Muted] {
                    assert!(contrast(value(foreground), value(background)) >= 4.5);
                }
            }
            assert!(contrast(0xffffff, value(Role::Accent)) >= 4.5);
            assert!(contrast(value(Role::Accent), value(Role::Background)) >= 3.0);
            assert!(contrast(value(Role::Danger), value(Role::Surface)) >= 4.5);
            for role in [Role::Danger, Role::Link, Role::Warning, Role::Muted] {
                for background in [Role::Surface, Role::Selected] {
                    let blend = |shift: u32| {
                        let foreground = (value(role) >> shift) & 255u32;
                        let background = (value(background) >> shift) & 255u32;
                        (foreground as f64 * 0.08 + background as f64 * 0.92).round() as u32
                    };
                    let badge = (blend(16) << 16) | (blend(8) << 8) | blend(0);
                    assert!(contrast(value(role), badge) >= 4.5);
                }
            }
            assert!(contrast(value(Role::Link), value(Role::Selected)) >= 3.0);
        }
    }
    #[test]
    fn appearance_values_round_trip_and_default_to_dark() {
        for theme in [Theme::Dark, Theme::Light, Theme::System] {
            assert_eq!(Theme::parse(theme.value()), theme);
        }
        assert_eq!(Theme::parse(""), Theme::Dark);
        assert_eq!(Theme::parse("unknown"), Theme::Dark);
    }
}
