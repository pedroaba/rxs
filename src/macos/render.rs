use crate::document::{Annotation, Document, Tool};
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::*;
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSData, NSDictionary, NSPoint, NSRect, NSSize};
use std::path::Path;

pub struct NativeImage {
    pub bitmap: Retained<NSBitmapImageRep>,
    pub width: f64,
    pub height: f64,
}

impl NativeImage {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Não foi possível ler a captura: {e}"))?;
        let data = NSData::with_bytes(&bytes);
        let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &data)
            .ok_or("A captura não contém uma imagem válida.")?;
        let width = bitmap.pixelsWide() as f64;
        let height = bitmap.pixelsHigh() as f64;
        if width <= 0.0 || height <= 0.0 {
            return Err("A captura está vazia.".into());
        }
        bitmap.setSize(NSSize::new(width, height));
        Ok(Self {
            bitmap,
            width,
            height,
        })
    }
}

pub fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// Draw in image-pixel coordinates with the origin at the top left.
pub fn draw(image: &NativeImage, document: &Document, draft: Option<&Annotation>) {
    let context = NSGraphicsContext::currentContext().expect("drawing requires a graphics context");
    let cg = context.CGContext();
    CGContext::save_g_state(Some(&cg));
    CGContext::translate_ctm(Some(&cg), 0.0, image.height);
    CGContext::scale_ctm(Some(&cg), 1.0, -1.0);
    if let Some(bitmap) = image.bitmap.CGImage() {
        CGContext::draw_image(
            Some(&cg),
            rect(0.0, 0.0, image.width, image.height),
            Some(&bitmap),
        );
    }
    CGContext::restore_g_state(Some(&cg));
    for annotation in document.annotations().chain(draft) {
        draw_annotation(annotation);
    }
}

fn draw_annotation(annotation: &Annotation) {
    if annotation.points.len() < 2 {
        return;
    }
    let [r, g, b, a] = annotation.style.color;
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a).setStroke();
    let path = NSBezierPath::bezierPath();
    path.setLineWidth(annotation.style.width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    let first = annotation.points[0];
    let last = *annotation.points.last().unwrap();
    if annotation.tool == Tool::Rectangle {
        path.appendBezierPathWithRect(rect(
            first.x.min(last.x),
            first.y.min(last.y),
            (last.x - first.x).abs(),
            (last.y - first.y).abs(),
        ));
    } else {
        path.moveToPoint(NSPoint::new(first.x, first.y));
        for point in &annotation.points[1..] {
            path.lineToPoint(NSPoint::new(point.x, point.y));
        }
        if annotation.tool == Tool::Arrow
            && let Some(head) = annotation.arrow_head()
        {
            path.moveToPoint(NSPoint::new(head[0].x, head[0].y));
            path.lineToPoint(NSPoint::new(head[1].x, head[1].y));
            path.lineToPoint(NSPoint::new(head[2].x, head[2].y));
        }
    }
    path.stroke();
}

pub fn new_bitmap(width: usize, height: usize) -> Result<Retained<NSBitmapImageRep>, String> {
    unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), width as isize, height as isize, 8, 4, true, false, NSDeviceRGBColorSpace, 0, 0,
        ).ok_or_else(|| "Não há memória suficiente para exportar esta imagem.".into())
    }
}

/// Restore the previous context even when exporting fails.
struct GraphicsState;
impl GraphicsState {
    fn save() -> Self {
        NSGraphicsContext::saveGraphicsState_class();
        Self
    }
}
impl Drop for GraphicsState {
    fn drop(&mut self) {
        NSGraphicsContext::restoreGraphicsState_class();
    }
}

pub fn render_bitmap(
    image: &NativeImage,
    document: &Document,
) -> Result<Retained<NSBitmapImageRep>, String> {
    let bitmap = new_bitmap(image.width as usize, image.height as usize)?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)
        .ok_or("Não foi possível criar o contexto de desenho.")?;
    let _restore = GraphicsState::save();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let cg = context.CGContext();
    CGContext::translate_ctm(Some(&cg), 0.0, image.height);
    CGContext::scale_ctm(Some(&cg), 1.0, -1.0);
    draw(image, document, None);
    Ok(bitmap)
}

pub fn png(bitmap: &NSBitmapImageRep) -> Result<Retained<NSData>, String> {
    let properties = NSDictionary::<_, AnyObject>::new();
    unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties) }
        .ok_or_else(|| "Não foi possível codificar a imagem PNG.".into())
}

pub fn export(image: &NativeImage, document: &Document) -> Result<Retained<NSData>, String> {
    let bitmap = render_bitmap(image, document)?;
    png(&bitmap)
}
