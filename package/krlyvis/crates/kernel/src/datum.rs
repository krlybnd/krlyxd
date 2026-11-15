//! What a statement of the vis layer makes: a picture, the outline of its subject, or that
//! outline placed in the world by a frame.

use crate::frame::{Frame, AXIS_NAMES};
use crate::image::{Gray, Pixel};
use crate::ports::outline::Shape;
use krly3d_core::schema::Value;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Datum {
    Picture { path: String, image: Arc<Gray> },
    Outline { shape: Arc<Shape> },
    Placed { shape: Arc<Shape>, frame: Frame },
}

impl Datum {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Picture { .. } => "picture",
            Self::Outline { .. } => "outline",
            Self::Placed { .. } => "placed outline",
        }
    }

    /// The outline this datum holds, if it holds one.
    pub fn shape(&self) -> Option<&Arc<Shape>> {
        match self {
            Self::Picture { .. } => None,
            Self::Outline { shape } | Self::Placed { shape, .. } => Some(shape),
        }
    }

    /// A number a claim can be about: `polygons`, `points`, `pixels`, and for a placed
    /// outline `width`, `height` and `area` in world units (in pixels otherwise).
    pub fn measure(&self, name: &str) -> Option<f64> {
        if let Self::Picture { image, .. } = self {
            return match name {
                "width" => Some(image.width() as f64),
                "height" => Some(image.height() as f64),
                _ => None,
            };
        }
        let shape = self.shape()?;
        let outer = shape.polygons.first()?;
        let world: Vec<[f64; 2]> = match self {
            Self::Placed { frame, .. } => outer.iter().map(|pixel| frame.world(pixel)).collect(),
            _ => outer.iter().map(|pixel| [pixel.x, pixel.y]).collect(),
        };
        let span = |k: usize| {
            let (low, high) = world.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                (low.min(point[k]), high.max(point[k]))
            });
            high - low
        };
        match name {
            "polygons" => Some(shape.polygons.len() as f64),
            "points" => Some(shape.polygons.iter().map(Vec::len).sum::<usize>() as f64),
            "pixels" => Some(shape.pixels as f64),
            "width" => Some(span(0)),
            "height" => Some(span(1)),
            "area" => Some(
                area(&world)
                    - shape
                        .polygons
                        .iter()
                        .skip(1)
                        .map(|polygon| {
                            let points: Vec<_> = polygon
                                .iter()
                                .map(|pixel| match self {
                                    Self::Placed { frame, .. } => frame.world(pixel),
                                    _ => [pixel.x, pixel.y],
                                })
                                .collect();
                            area(&points)
                        })
                        .sum::<f64>(),
            ),
            _ => None,
        }
    }

    /// For a placed outline: across the line where world axis `axis` equals `at`, the
    /// lowest and highest world coordinate of the outline along its other axis, with that
    /// axis. Nothing where the line misses the outline or the outline does not show `axis`.
    pub fn extent_at(&self, axis: usize, at: f64) -> Option<(usize, [f64; 2])> {
        let Self::Placed { shape, frame } = self else { return None };
        let along = frame.axes.iter().position(|shown| *shown == axis)?;
        let across = 1 - along;
        let mut extent: Option<[f64; 2]> = None;
        for polygon in &shape.polygons {
            let world: Vec<[f64; 2]> = polygon.iter().map(|pixel| frame.world(pixel)).collect();
            for i in 0..world.len() {
                let (a, b) = (world[i], world[(i + 1) % world.len()]);
                if (a[along] <= at) == (b[along] <= at) {
                    continue;
                }
                let crossing = a[across] + (at - a[along]) / (b[along] - a[along]) * (b[across] - a[across]);
                extent = Some(
                    extent
                        .map_or([crossing, crossing], |[low, high]| [low.min(crossing), high.max(crossing)]),
                );
            }
        }
        extent.map(|found| (frame.axes[across], found))
    }

    /// The datum as data for a client, every coordinate included.
    pub fn value(&self) -> Value {
        let pixels = |polygon: &[Pixel]| -> Value {
            Value::List(polygon.iter().map(|pixel| pair([pixel.x, pixel.y])).collect())
        };
        match self {
            Self::Picture { path, image } => Value::map([
                ("kind", Value::from("picture")),
                ("path", Value::from(path.as_str())),
                ("size", pair([image.width() as f64, image.height() as f64])),
            ]),
            Self::Outline { shape } => Value::map([
                ("kind", Value::from("outline")),
                ("polygons", Value::List(shape.polygons.iter().map(|polygon| pixels(polygon)).collect())),
                ("threshold", Value::from(shape.threshold)),
            ]),
            Self::Placed { shape, frame } => {
                let world = |polygon: &[Pixel]| -> Value {
                    Value::List(polygon.iter().map(|pixel| pair(frame.world(pixel))).collect())
                };
                Value::map([
                    ("kind", Value::from("placed")),
                    ("view", Value::from(frame.view.name())),
                    ("looking", Value::from(frame.view.looking())),
                    (
                        "axes",
                        Value::List(frame.axes.iter().map(|axis| Value::from(AXIS_NAMES[*axis])).collect()),
                    ),
                    ("unit", Value::from(frame.unit())),
                    ("material_area", Value::from(self.measure("area").unwrap_or(0.0))),
                    // Coordinates are a polyline derived from pixels (possibly simplified),
                    // never a declaration of subpixel measurement accuracy. The complete
                    // polygons/pixels below are the replayable reference snapshot.
                    (
                        "evidence",
                        Value::map([
                            ("kind", "raster".into()),
                            ("source", "krlyvis.placed-pixel-outline/1".into()),
                            ("uncertainty", "unknown".into()),
                            ("pixel_unit", frame.unit().into()),
                            ("threshold", shape.threshold.into()),
                            (
                                "coordinates",
                                "pixel-derived polyline; simplification and image measurement error unknown"
                                    .into(),
                            ),
                        ]),
                    ),
                    ("polygons", Value::List(shape.polygons.iter().map(|polygon| world(polygon)).collect())),
                    ("pixels", Value::List(shape.polygons.iter().map(|polygon| pixels(polygon)).collect())),
                ])
            }
        }
    }
}

fn pair(numbers: [f64; 2]) -> Value {
    Value::List(numbers.iter().map(|number| Value::from(*number)).collect())
}

/// The area a closed polygon encloses, whichever way it runs.
fn area(polygon: &[[f64; 2]]) -> f64 {
    let doubled: f64 = (0..polygon.len())
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    0.5 * doubled.abs()
}

#[cfg(test)]
mod tests {
    use super::Datum;
    use crate::frame::{Frame, View};
    use crate::image::Pixel;
    use crate::ports::outline::Shape;
    use std::sync::Arc;

    #[test]
    fn material_area_subtracts_holes_and_scales_with_frame() {
        let rect = |low: f64, high: f64| {
            vec![Pixel::new(low, low), Pixel::new(high, low), Pixel::new(high, high), Pixel::new(low, high)]
        };
        let shape = Arc::new(Shape {
            polygons: vec![rect(0.0, 4.0), rect(1.0, 3.0)],
            pixels: 12,
            threshold: 0.5,
            dark: true,
        });
        assert_eq!(Datum::Outline { shape: Arc::clone(&shape) }.measure("area"), Some(12.0));
        let frame = Frame::through(
            View::Top,
            [0, 1],
            (Pixel::new(0.0, 0.0), [0.0, 0.0]),
            (Pixel::new(4.0, 4.0), [1.0, 1.0]),
        )
        .unwrap();
        assert_eq!(Datum::Placed { shape, frame }.measure("area"), Some(0.75));
    }
    #[test]
    fn a_placed_outline_tells_its_extent_across_a_station() {
        // A diamond of pixels, placed one pixel to one unit in a side view.
        let corners = [(10.0, 0.0), (20.0, 10.0), (10.0, 20.0), (0.0, 10.0)];
        let polygon = corners.iter().map(|&(x, y)| Pixel::new(x, y)).collect();
        let shape = Arc::new(Shape { polygons: vec![polygon], pixels: 200, threshold: 128.0, dark: true });
        let frame = Frame::through(
            View::Side,
            [0, 2],
            (Pixel::new(0.0, 0.0), [0.0, 0.0]),
            (Pixel::new(10.0, 10.0), [10.0, -10.0]),
        )
        .unwrap();
        let placed = Datum::Placed { shape, frame };

        let (axis, [low, high]) = placed.extent_at(0, 5.0).unwrap();

        assert_eq!(axis, 2);
        assert!((low + 15.0).abs() < 1e-9 && (high + 5.0).abs() < 1e-9, "{low} {high}");
        assert!(placed.extent_at(1, 5.0).is_none() && placed.extent_at(0, 25.0).is_none());
    }
}
