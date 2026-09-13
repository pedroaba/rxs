use std::collections::VecDeque;

pub const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
    pub fn to_image(self, zoom: f64, width: f64, height: f64) -> Self {
        Self::new(
            (self.x / zoom).clamp(0.0, width),
            (self.y / zoom).clamp(0.0, height),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Rectangle,
    Freehand,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: [f64; 4],
    pub width: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: [0.95, 0.18, 0.16, 1.0],
            width: 4.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub tool: Tool,
    pub style: Style,
    pub points: Vec<Point>,
}

impl Annotation {
    pub fn new(tool: Tool, style: Style, point: Point) -> Self {
        Self {
            tool,
            style,
            points: vec![point],
        }
    }
    pub fn update(&mut self, point: Point) {
        if self.tool == Tool::Freehand {
            // Keep a bounded number of points per stroke and suppress subpixel jitter.
            if self.points.len() < 100_000 && self.points.last().unwrap().distance(point) >= 0.75 {
                self.points.push(point);
            }
        } else if self.points.len() == 1 {
            self.points.push(point);
        } else {
            self.points[1] = point;
        }
    }
    pub fn is_visible(&self) -> bool {
        self.points.len() >= 2
            && self
                .points
                .iter()
                .skip(1)
                .any(|p| self.points[0].distance(*p) >= 1.0)
    }
    pub fn arrow_head(&self) -> Option<[Point; 3]> {
        let start = *self.points.first()?;
        let end = *self.points.last()?;
        let length = start.distance(end);
        if length < 1.0 {
            return None;
        }
        let ux = (end.x - start.x) / length;
        let uy = (end.y - start.y) / length;
        let head = (self.style.width * 4.0).max(12.0).min(length * 0.65);
        Some([
            Point::new(
                end.x - ux * head - uy * head * 0.45,
                end.y - uy * head + ux * head * 0.45,
            ),
            end,
            Point::new(
                end.x - ux * head + uy * head * 0.45,
                end.y - uy * head - ux * head * 0.45,
            ),
        ])
    }
}

struct Entry {
    annotation: Annotation,
    revision: u64,
}

/// The image is owned by the platform renderer. History stores vectors only.
pub struct Document {
    pub width: f64,
    pub height: f64,
    pub tool: Tool,
    pub style: Style,
    committed: Vec<Annotation>,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    revision: u64,
    next_revision: u64,
    baseline_revision: u64,
    exported_revision: Option<u64>,
}

impl Document {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            tool: Tool::Arrow,
            style: Style::default(),
            committed: vec![],
            undo: VecDeque::new(),
            redo: vec![],
            revision: 0,
            next_revision: 1,
            baseline_revision: 0,
            exported_revision: None,
        }
    }
    pub fn annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.committed
            .iter()
            .chain(self.undo.iter().map(|e| &e.annotation))
    }
    pub fn push(&mut self, annotation: Annotation) {
        if !annotation.is_visible() {
            return;
        }
        self.redo.clear();
        self.revision = self.next_revision;
        self.next_revision += 1;
        self.undo.push_back(Entry {
            annotation,
            revision: self.revision,
        });
        if self.undo.len() > HISTORY_LIMIT {
            let entry = self.undo.pop_front().unwrap();
            self.baseline_revision = entry.revision;
            self.committed.push(entry.annotation);
        }
    }
    pub fn undo(&mut self) {
        if let Some(entry) = self.undo.pop_back() {
            self.redo.push(entry);
            self.revision = self
                .undo
                .back()
                .map_or(self.baseline_revision, |e| e.revision);
        }
    }
    pub fn redo(&mut self) {
        if let Some(entry) = self.redo.pop() {
            self.revision = entry.revision;
            self.undo.push_back(entry);
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn mark_exported(&mut self) {
        self.exported_revision = Some(self.revision);
    }
    pub fn needs_export(&self) -> bool {
        self.exported_revision != Some(self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arrow(x: f64) -> Annotation {
        let mut a = Annotation::new(Tool::Arrow, Style::default(), Point::new(x, 0.0));
        a.update(Point::new(x + 100.0, 0.0));
        a
    }
    #[test]
    fn coordinates_are_image_pixels_at_every_zoom() {
        assert_eq!(
            Point::new(100.0, 50.0).to_image(0.5, 3840.0, 2160.0),
            Point::new(200.0, 100.0)
        );
        assert_eq!(
            Point::new(-10.0, 5000.0).to_image(2.0, 100.0, 100.0),
            Point::new(0.0, 100.0)
        );
    }
    #[test]
    fn arrow_head_handles_directions_and_short_strokes() {
        let a = arrow(0.0);
        let head = a.arrow_head().unwrap();
        assert_eq!(head[1], Point::new(100.0, 0.0));
        assert_eq!(head[0].y, -head[2].y);
        assert!(head[0].x < head[1].x);
        let mut tiny = arrow(0.0);
        tiny.points[1] = Point::new(0.0, 0.0);
        assert!(tiny.arrow_head().is_none());
    }
    #[test]
    fn export_state_survives_undo_and_detects_a_new_branch() {
        let mut doc = Document::new(100.0, 100.0);
        assert!(doc.needs_export());
        doc.mark_exported();
        doc.push(arrow(0.0));
        assert!(doc.needs_export());
        doc.undo();
        assert!(!doc.needs_export());
        doc.redo();
        doc.mark_exported();
        doc.undo();
        doc.push(arrow(20.0));
        assert!(doc.needs_export());
        assert!(!doc.can_redo());
    }
    #[test]
    fn bounded_history_preserves_old_artwork() {
        let mut doc = Document::new(100.0, 100.0);
        for i in 0..150 {
            doc.push(arrow(i as f64));
        }
        assert_eq!(doc.undo.len(), HISTORY_LIMIT);
        for _ in 0..150 {
            doc.undo();
        }
        assert_eq!(doc.annotations().count(), 50);
        for _ in 0..100 {
            doc.redo();
        }
        assert_eq!(doc.annotations().count(), 150);
    }
    #[test]
    fn click_without_drag_does_not_create_history() {
        let mut doc = Document::new(100.0, 100.0);
        doc.push(Annotation::new(
            Tool::Rectangle,
            Style::default(),
            Point::default(),
        ));
        assert!(!doc.can_undo());
    }
}
