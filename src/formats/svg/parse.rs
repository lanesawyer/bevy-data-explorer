//! Reading outlines out of an SVG.
//!
//! Not an SVG renderer. Annotation tools export regions as polygons in the
//! pixel space of the slide they were drawn on, with a label or two in custom
//! attributes, and that is what is read: every outline, its stroke, and the
//! labels that say what it outlines. Fills, filters and text are not drawn.
//!
//! Anything that would put an outline somewhere other than where its points
//! say — a `transform`, or a curve in a path — is skipped and counted rather
//! than drawn in the wrong place, which would be worse than not drawing it.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// Every outline in a document, in the document's own order.
#[derive(Debug, Clone)]
pub struct Svg {
    pub name: String,
    /// The size the document declares, which outlines are scaled into.
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
    /// Groups the outlines were found in, by id.
    pub groups: usize,
    /// Elements that would have been drawn but could not be read faithfully.
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// In document pixels, y down.
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
    /// Straight RGBA.
    pub color: [f32; 4],
    pub width: f32,
    /// Length of a dash, or zero for a solid line.
    pub dash: f32,
    /// What the shape outlines: its id and any attribute named `*-label`, in
    /// document order.
    pub labels: Vec<(String, String)>,
    /// The id of the nearest group holding it.
    pub group: Option<String>,
}

impl Shape {
    /// What to call this outline: a structure's label if it has one, then any
    /// other label, then its id, then its group's.
    pub fn title(&self) -> Option<&str> {
        let find = |wanted: &dyn Fn(&str) -> bool| {
            self.labels
                .iter()
                .find(|(key, _)| wanted(key))
                .map(|(_, value)| value.as_str())
        };
        find(&|key| key == "structure-label")
            .or_else(|| find(&|key| key.ends_with("-label")))
            .or_else(|| find(&|key| key == "id"))
            .or(self.group.as_deref())
    }

    /// The area enclosed, which picks the innermost of nested regions.
    pub fn area(&self) -> f32 {
        let n = self.points.len();
        let twice: f32 = (0..n)
            .map(|i| {
                let ([x0, y0], [x1, y1]) = (self.points[i], self.points[(i + 1) % n]);
                x0 * y1 - x1 * y0
            })
            .sum();
        (twice * 0.5).abs()
    }

    /// Whether a point lies inside the outline, by the even-odd rule.
    pub fn contains(&self, [x, y]: [f32; 2]) -> bool {
        if !self.closed || self.points.len() < 3 {
            return false;
        }
        let mut inside = false;
        let mut previous = self.points[self.points.len() - 1];
        for &point in &self.points {
            let ([xi, yi], [xj, yj]) = (point, previous);
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            previous = point;
        }
        inside
    }

    /// Distance from a point to the nearest part of the outline itself.
    pub fn distance_to_outline(&self, [x, y]: [f32; 2]) -> f32 {
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        (0..segments)
            .map(|i| {
                let ([ax, ay], [bx, by]) = (self.points[i], self.points[(i + 1) % n]);
                let (dx, dy) = (bx - ax, by - ay);
                let length = dx * dx + dy * dy;
                let t = if length > 0.0 {
                    (((x - ax) * dx + (y - ay) * dy) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (px, py) = (ax + t * dx - x, ay + t * dy - y);
                (px * px + py * py).sqrt()
            })
            .fold(f32::INFINITY, f32::min)
    }
}

/// What an element inherits from the groups around it.
#[derive(Debug, Clone)]
struct Inherited {
    stroke: Option<Paint>,
    fill: Option<Paint>,
    width: f32,
    dash: f32,
    stroke_opacity: f32,
    opacity: f32,
    group: Option<String>,
    /// Somewhere above this element a transform moved everything.
    transformed: bool,
}

impl Default for Inherited {
    fn default() -> Self {
        Inherited {
            stroke: None,
            // SVG fills black when nothing says otherwise.
            fill: Some(Paint::Color([0.0, 0.0, 0.0])),
            width: 1.0,
            dash: 0.0,
            stroke_opacity: 1.0,
            opacity: 1.0,
            group: None,
            transformed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Paint {
    None,
    Color([f32; 3]),
}

/// Maps document user units onto the declared size.
#[derive(Debug, Clone, Copy)]
struct Viewport {
    scale: [f32; 2],
    offset: [f32; 2],
}

impl Viewport {
    fn map(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            (x - self.offset[0]) * self.scale[0],
            (y - self.offset[1]) * self.scale[1],
        ]
    }

    fn length(&self, length: f32) -> f32 {
        length * self.scale[0]
    }
}

pub fn parse(name: &str, text: &str) -> Result<Svg, String> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Inherited> = Vec::new();
    let mut svg: Option<(Svg, Viewport)> = None;
    let mut groups = std::collections::HashSet::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("{name} is not well-formed XML: {e}"))?;
        let (element, empty) = match &event {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::End(_) => {
                stack.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };

        let attributes = attributes(element)?;
        let local = element.local_name();
        let tag = std::str::from_utf8(local.as_ref()).unwrap_or_default();
        let parent = stack.last().cloned().unwrap_or_default();

        if tag == "svg" && svg.is_none() {
            svg = Some(document(name, &attributes)?);
        }
        let mut inherited = inherit(&parent, &attributes);
        if tag == "g"
            && let Some(id) = value(&attributes, "id")
        {
            inherited.group = Some(id.to_string());
        }

        if let Some((document, viewport)) = svg.as_mut()
            && let Some(outline) = outline(tag, &attributes)
        {
            match outline {
                Some((points, closed)) if !inherited.transformed && points.len() >= 2 => {
                    if let Some(group) = &inherited.group {
                        groups.insert(group.clone());
                    }
                    if let Some(shape) = shape(points, closed, &inherited, &attributes, viewport) {
                        document.shapes.push(shape);
                    }
                }
                Some(_) if !inherited.transformed => {}
                _ => document.skipped += 1,
            }
        }

        if !empty {
            stack.push(inherited);
        }
    }

    let (mut svg, _) = svg.ok_or_else(|| format!("{name} has no <svg> element"))?;
    svg.groups = groups.len();
    Ok(svg)
}

fn attributes(element: &BytesStart) -> Result<Vec<(String, String)>, String> {
    element
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|e| e.to_string())?;
            let key = String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned();
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| e.to_string())?;
            Ok((key, value.into_owned()))
        })
        .collect()
}

fn value<'a>(attributes: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

/// The document's declared size, and how its user units map onto it.
fn document(name: &str, attributes: &[(String, String)]) -> Result<(Svg, Viewport), String> {
    let view_box: Option<Vec<f32>> = value(attributes, "viewBox")
        .map(numbers)
        .filter(|numbers| numbers.len() == 4 && numbers[2] > 0.0 && numbers[3] > 0.0);
    let declared = |key| value(attributes, key).and_then(leading_number);

    let (width, height) = match (declared("width"), declared("height"), &view_box) {
        (Some(width), Some(height), _) => (width, height),
        (_, _, Some(view_box)) => (view_box[2], view_box[3]),
        _ => return Err(format!("{name} declares neither a size nor a viewBox")),
    };
    let viewport = match view_box {
        Some(view_box) => Viewport {
            scale: [width / view_box[2], height / view_box[3]],
            offset: [view_box[0], view_box[1]],
        },
        None => Viewport {
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        },
    };
    Ok((
        Svg {
            name: name.to_string(),
            width,
            height,
            shapes: Vec::new(),
            groups: 0,
            skipped: 0,
        },
        viewport,
    ))
}

/// What an element's style says, over what it inherited.
fn inherit(parent: &Inherited, attributes: &[(String, String)]) -> Inherited {
    let mut style = parent.clone();
    style.opacity = 1.0;
    if value(attributes, "transform").is_some() {
        style.transformed = true;
    }

    // Presentation attributes first, then the style attribute, which wins.
    let declarations = attributes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .chain(
            value(attributes, "style")
                .into_iter()
                .flat_map(|style| style.split(';'))
                .filter_map(|declaration| declaration.split_once(':'))
                .map(|(key, value)| (key.trim(), value.trim())),
        );
    for (key, value) in declarations {
        match key {
            "stroke" => style.stroke = paint(value).or(style.stroke),
            "fill" => style.fill = paint(value).or(style.fill),
            "stroke-width" => style.width = leading_number(value).unwrap_or(style.width),
            "stroke-dasharray" => {
                style.dash = numbers(value).first().copied().unwrap_or(0.0);
            }
            "stroke-opacity" => {
                style.stroke_opacity = leading_number(value).unwrap_or(1.0);
            }
            "opacity" => style.opacity = leading_number(value).unwrap_or(1.0),
            _ => {}
        }
    }
    style.opacity *= parent.opacity;
    style
}

/// The points an element outlines, `Some(None)` for a drawable element that
/// cannot be read faithfully, and `None` for anything that draws no outline.
#[expect(
    clippy::option_option,
    reason = "not an outline, and an outline that could not be read, are different answers"
)]
fn outline(tag: &str, attributes: &[(String, String)]) -> Option<Option<(Vec<[f32; 2]>, bool)>> {
    let number = |key| value(attributes, key).and_then(leading_number);
    let pairs = |key| {
        let values = numbers(value(attributes, key).unwrap_or_default());
        values.as_chunks::<2>().0.to_vec()
    };
    Some(match tag {
        "polygon" => Some((pairs("points"), true)),
        "polyline" => Some((pairs("points"), false)),
        "line" => Some((
            vec![
                [number("x1").unwrap_or(0.0), number("y1").unwrap_or(0.0)],
                [number("x2").unwrap_or(0.0), number("y2").unwrap_or(0.0)],
            ],
            false,
        )),
        "rect" => {
            let (x, y) = (number("x").unwrap_or(0.0), number("y").unwrap_or(0.0));
            let (w, h) = (number("width")?, number("height")?);
            Some((vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true))
        }
        "path" => straight_path(value(attributes, "d").unwrap_or_default()),
        "circle" | "ellipse" => None,
        _ => return None,
    })
}

/// A path made only of straight segments, as annotation exports write them.
/// Anything with a curve in it is refused rather than straightened.
fn straight_path(d: &str) -> Option<(Vec<[f32; 2]>, bool)> {
    let mut points = Vec::new();
    let mut closed = false;
    let mut at = [0.0f32, 0.0];
    let mut tokens = path_tokens(d).into_iter().peekable();

    while let Some(token) = tokens.next() {
        let PathToken::Command(command) = token else {
            return None;
        };
        if command.eq_ignore_ascii_case(&'z') {
            closed = true;
            continue;
        }
        let mut args = Vec::new();
        while let Some(PathToken::Number(n)) = tokens.peek() {
            args.push(*n);
            tokens.next();
        }
        let relative = command.is_ascii_lowercase();
        let origin = |at: [f32; 2]| if relative { at } else { [0.0, 0.0] };
        match command.to_ascii_uppercase() {
            'M' | 'L' => {
                for pair in args.as_chunks::<2>().0 {
                    let base = origin(at);
                    at = [base[0] + pair[0], base[1] + pair[1]];
                    points.push(at);
                }
            }
            'H' => {
                for x in args {
                    at[0] = origin(at)[0] + x;
                    points.push(at);
                }
            }
            'V' => {
                for y in args {
                    at[1] = origin(at)[1] + y;
                    points.push(at);
                }
            }
            _ => return None,
        }
    }
    Some((points, closed))
}

enum PathToken {
    Command(char),
    Number(f32),
}

fn path_tokens(d: &str) -> Vec<PathToken> {
    let mut tokens = Vec::new();
    let mut rest = d;
    while let Some(c) = rest.chars().next() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            tokens.push(PathToken::Command(c));
            rest = &rest[1..];
        } else if let Some((number, len)) = scan_number(rest) {
            tokens.push(PathToken::Number(number));
            rest = &rest[len..];
        } else {
            rest = &rest[c.len_utf8()..];
        }
    }
    tokens
}

/// A number at the start of `text`, and how many bytes it took.
fn scan_number(text: &str) -> Option<(f32, usize)> {
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'-' | b'+')) {
        end += 1;
    }
    let mut seen_dot = false;
    let mut digits = 0;
    while let Some(&b) = bytes.get(end) {
        match b {
            b'0'..=b'9' => digits += 1,
            b'.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        end += 1;
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'-' | b'+')) {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            end = exponent;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
        }
    }
    text[..end].parse().ok().map(|number| (number, end))
}

fn numbers(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match scan_number(rest) {
            Some((number, len)) => {
                out.push(number);
                rest = &rest[len..];
            }
            None => rest = &rest[rest.chars().next().map_or(1, char::len_utf8)..],
        }
    }
    out
}

/// The number a length starts with, ignoring any unit after it.
fn leading_number(text: &str) -> Option<f32> {
    scan_number(text.trim()).map(|(number, _)| number)
}

fn paint(value: &str) -> Option<Paint> {
    let value = value.trim().to_ascii_lowercase();
    if value == "none" || value == "transparent" {
        return Some(Paint::None);
    }
    if let Some(hex) = value.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        let channel = |high: u8, low: u8| f32::from(high * 16 + low) / 255.0;
        return match digits.as_slice() {
            [r, g, b] => Some(Paint::Color([
                channel(*r, *r),
                channel(*g, *g),
                channel(*b, *b),
            ])),
            [r1, r2, g1, g2, b1, b2] => Some(Paint::Color([
                channel(*r1, *r2),
                channel(*g1, *g2),
                channel(*b1, *b2),
            ])),
            _ => None,
        };
    }
    if let Some(inner) = value
        .strip_prefix("rgb(")
        .or_else(|| value.strip_prefix("rgba("))
        .and_then(|inner| inner.strip_suffix(')'))
    {
        let channels: Vec<f32> = inner
            .split(',')
            .take(3)
            .map(|part| {
                let part = part.trim();
                match part.strip_suffix('%') {
                    Some(percent) => percent.parse::<f32>().ok().map(|p| p / 100.0),
                    None => part.parse::<f32>().ok().map(|c| c / 255.0),
                }
            })
            .collect::<Option<_>>()?;
        return match channels.as_slice() {
            [r, g, b] => Some(Paint::Color([*r, *g, *b])),
            _ => None,
        };
    }
    let named = match value.as_str() {
        "black" => [0.0, 0.0, 0.0],
        "white" => [1.0, 1.0, 1.0],
        "red" => [1.0, 0.0, 0.0],
        "lime" => [0.0, 1.0, 0.0],
        "green" => [0.0, 0.5, 0.0],
        "blue" => [0.0, 0.0, 1.0],
        "yellow" => [1.0, 1.0, 0.0],
        "cyan" | "aqua" => [0.0, 1.0, 1.0],
        "magenta" | "fuchsia" => [1.0, 0.0, 1.0],
        "orange" => [1.0, 0.65, 0.0],
        "gray" | "grey" => [0.5, 0.5, 0.5],
        _ => return None,
    };
    Some(Paint::Color(named))
}

fn shape(
    points: Vec<[f32; 2]>,
    closed: bool,
    style: &Inherited,
    attributes: &[(String, String)],
    viewport: &Viewport,
) -> Option<Shape> {
    // An outline with no stroke is drawn in its fill instead: a region filled
    // and not stroked would otherwise be an annotation nobody can see.
    let (rgb, opacity) = match (style.stroke, style.fill) {
        (Some(Paint::Color(rgb)), _) => (rgb, style.stroke_opacity),
        (_, Some(Paint::Color(rgb))) => (rgb, 1.0),
        _ => return None,
    };
    let labels = attributes
        .iter()
        .filter(|(key, _)| key == "id" || key.ends_with("-label"))
        .cloned()
        .collect();
    Some(Shape {
        points: points
            .into_iter()
            .map(|point| viewport.map(point))
            .collect(),
        closed,
        color: [rgb[0], rgb[1], rgb[2], opacity * style.opacity],
        width: viewport.length(style.width),
        dash: viewport.length(style.dash),
        labels,
        group: style.group.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Svg {
        parse(
            "annotation",
            include_str!("../../../testdata/annotation.svg"),
        )
        .unwrap()
    }

    #[test]
    fn the_document_is_as_large_as_the_slide_it_was_drawn_on() {
        let svg = fixture();
        assert_eq!((svg.width, svg.height), (15936.0, 11526.0));
    }

    #[test]
    fn every_polygon_is_an_outline_with_its_labels() {
        let svg = fixture();
        // Two structures, one outlined twice: solid, then dashed over it.
        assert_eq!(svg.shapes.len(), 3);
        assert_eq!(svg.groups, 2);
        assert_eq!(svg.skipped, 0);

        let first = &svg.shapes[0];
        assert!(first.closed);
        assert_eq!(first.points[0], [7179.25, 4301.5]);
        assert_eq!(first.title(), Some("DG-R"));
        assert!(
            first
                .labels
                .contains(&("region-label".to_string(), "HIP-MEC".to_string()))
        );
        assert_eq!(first.group.as_deref(), Some("DG-R"));
    }

    #[test]
    fn style_declarations_set_the_stroke() {
        let svg = fixture();
        let solid = &svg.shapes[0];
        assert_eq!(solid.width, 10.0);
        assert_eq!(solid.dash, 0.0);
        let expected = [149.0 / 255.0, 179.0 / 255.0, 215.0 / 255.0, 1.0];
        for (got, want) in solid.color.iter().zip(expected) {
            assert!((got - want).abs() < 1e-6);
        }

        let dashed = &svg.shapes[1];
        assert_eq!(dashed.color, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(dashed.dash, 40.0);
    }

    #[test]
    fn a_view_box_scales_outlines_into_the_declared_size() {
        let svg = parse(
            "scaled",
            r#"<svg width="200" height="100" viewBox="10 10 20 10">
                 <polyline points="10,10 30,20" stroke="red" stroke-width="2" />
               </svg>"#,
        )
        .unwrap();
        let shape = &svg.shapes[0];
        assert_eq!(shape.points, vec![[0.0, 0.0], [200.0, 100.0]]);
        assert_eq!(shape.width, 20.0);
        assert!(!shape.closed);
    }

    #[test]
    fn a_moved_or_curved_outline_is_counted_rather_than_misplaced() {
        let svg = parse(
            "skips",
            r##"<svg width="10" height="10">
                 <g transform="translate(5,5)"><polygon points="0,0 1,0 1,1" stroke="#fff" /></g>
                 <path d="M0 0 C 1 1 2 2 3 3" stroke="#fff" />
                 <path d="M0,0 l5,0 v5 h-5 z" stroke="#fff" />
                 <circle cx="1" cy="1" r="1" stroke="#fff" />
               </svg>"##,
        )
        .unwrap();
        assert_eq!(svg.skipped, 3);
        assert_eq!(svg.shapes.len(), 1);
        let square = &svg.shapes[0];
        assert!(square.closed);
        assert_eq!(
            square.points,
            vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]]
        );
    }

    #[test]
    fn an_unstroked_region_is_outlined_in_its_fill() {
        let svg = parse(
            "filled",
            r#"<svg width="10" height="10">
                 <rect x="1" y="1" width="2" height="2" fill="rgb(0,255,0)" />
                 <rect x="1" y="1" width="2" height="2" fill="none" />
               </svg>"#,
        )
        .unwrap();
        assert_eq!(svg.shapes.len(), 1);
        assert_eq!(svg.shapes[0].color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn colors_are_read_in_the_forms_exports_write() {
        assert_eq!(paint("#fff"), Some(Paint::Color([1.0, 1.0, 1.0])));
        assert_eq!(paint("#000000"), Some(Paint::Color([0.0, 0.0, 0.0])));
        assert_eq!(paint("rgb(255, 0, 0)"), Some(Paint::Color([1.0, 0.0, 0.0])));
        assert_eq!(paint("none"), Some(Paint::None));
        assert_eq!(paint("url(#gradient)"), None);
    }

    #[test]
    fn numbers_are_read_however_they_are_separated() {
        assert_eq!(numbers("1,2 3-4e1 .5"), vec![1.0, 2.0, 3.0, -40.0, 0.5]);
        assert_eq!(leading_number("10px"), Some(10.0));
    }

    #[test]
    fn the_innermost_region_under_a_point_is_the_smallest() {
        let outer = Shape {
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            closed: true,
            color: [1.0; 4],
            width: 1.0,
            dash: 0.0,
            labels: Vec::new(),
            group: None,
        };
        let inner = Shape {
            points: vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]],
            ..outer.clone()
        };
        assert!(outer.contains([5.0, 5.0]) && inner.contains([5.0, 5.0]));
        assert!(!inner.contains([1.0, 1.0]));
        assert!(inner.area() < outer.area());
        assert_eq!(outer.area(), 100.0);
        assert!((outer.distance_to_outline([5.0, 1.0]) - 1.0).abs() < 1e-6);
    }
}
