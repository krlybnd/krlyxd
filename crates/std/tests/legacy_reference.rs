//! The rebuilt kernel against the numbers recorded from the kernel it replaces
//! (`tests/reference/legacy-kernel.json`, measured before that code was removed).

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{Accuracy, Id};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::tool::run;
use krly3d_kernel::schema::Params;
use krly3d_kernel::topo::Body;
use krly3d_kernel::Context;
use std::f64::consts::TAU;
use std::sync::Arc;

const RECORDED: &str = include_str!("../../../tests/reference/legacy-kernel.json");

fn context() -> Context {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.context()
}

fn made(cx: &Context, tool: &str, params: Params, inputs: &[Arc<Body>]) -> Arc<Body> {
    Arc::new(run(cx, &Id::new(tool), &params, inputs).into_result().unwrap())
}

fn measured(cx: &Context, name: &str, body: &Arc<Body>) -> f64 {
    measure(cx, name, body).into_result().unwrap().value
}

/// The number recorded under `key` in the case named `case`.
fn recorded(case: &str, key: &str) -> f64 {
    let from = RECORDED.find(&format!("\"{case}\"")).unwrap_or_else(|| panic!("no recorded case `{case}`"));
    let label = format!("\"{key}\": ");
    let at = from + RECORDED[from..].find(&label).unwrap() + label.len();
    let end = at + RECORDED[at..].find([',', '\n']).unwrap();
    RECORDED[at..end].trim().parse().unwrap()
}

fn agrees(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0)
}

#[test]
fn primitives_have_the_recorded_volume_and_area() {
    let cx = context();
    let cases = [
        ("box 40x20x10", "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0)),
        ("cylinder r3 h10", "cylinder", Params::new().with("r", 3.0).with("h", 10.0)),
        ("sphere r5", "sphere", Params::new().with("r", 5.0)),
        ("cone r4 h9", "cone", Params::new().with("r", 4.0).with("h", 9.0)),
        ("frustum r4 r2 h9", "cone", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0)),
        ("torus R6 r2", "torus", Params::new().with("ring", 6.0).with("tube", 2.0)),
    ];
    for (case, tool, params) in cases {
        let body = made(&cx, tool, params, &[]);

        let (volume, area) = (measured(&cx, "volume", &body), measured(&cx, "area", &body));

        assert!(agrees(volume, recorded(case, "volume")), "{case}: volume {volume}");
        assert!(agrees(area, recorded(case, "area")), "{case}: area {area}");
    }
}

/// The fluted column shaft of the old example: a foot, a stretch with 24 flutes drawn as
/// one profile, and a neck. Flute k is a circle of radius `flute` centred `centre` from
/// the axis; it meets the shaft circle at the angles φk ± α with
/// cos α = (R² + d² − r²) / (2·R·d).
#[test]
fn the_fluted_shaft_has_the_recorded_volume() {
    let cx = context();
    let (radius, flute, centre, flutes) = (0.5_f64, 0.046_f64, 0.506_f64, 24);
    let (foot, fluted, neck) = (0.45, 7.35, 0.45);
    let alpha = ((radius * radius + centre * centre - flute * flute) / (2.0 * radius * centre)).acos();
    let on = |angle: f64| format!("{},{}", radius * angle.cos(), radius * angle.sin());
    let mut path = format!("M {}", on(-alpha));
    for k in 0..flutes {
        let phi = TAU * f64::from(k) / f64::from(flutes);
        // Into the shaft along the flute circle, then on along the shaft circle.
        path += &format!(" A {},{} {} cw", centre * phi.cos(), centre * phi.sin(), on(phi + alpha));
        path += &format!(" A 0,0 {} ccw", on(TAU * f64::from(k + 1) / f64::from(flutes) - alpha));
    }
    path += " Z";

    let outline = made(&cx, "profile", Params::new().with("path", path.as_str()), &[]);
    let parts = [
        made(&cx, "cylinder", Params::new().with("r", radius).with("h", foot), &[]),
        made(&cx, "extrude", Params::new().with("h", fluted), &[outline]),
        made(&cx, "cylinder", Params::new().with("r", radius).with("h", neck), &[]),
    ];
    let volume: f64 = parts.iter().map(|part| measured(&cx, "volume", part)).sum();

    let lens =
        flute * flute * ((centre * centre + flute * flute - radius * radius) / (2.0 * centre * flute)).acos()
            + radius * radius * alpha
            - 0.5
                * ((-centre + flute + radius)
                    * (centre + flute - radius)
                    * (centre - flute + radius)
                    * (centre + flute + radius))
                    .sqrt();
    let closed_form =
        std::f64::consts::PI * radius * radius * (foot + fluted + neck) - f64::from(flutes) * lens * fluted;
    assert!(agrees(volume, closed_form), "volume {volume} against the closed form {closed_form}");
    // The old kernel printed its result to nine decimals.
    let label = "shaft volume: kernel ";
    let at = RECORDED.find(label).unwrap() + label.len();
    let printed: f64 = RECORDED[at..].split_whitespace().next().unwrap().parse().unwrap();
    assert!((volume - printed).abs() < 5e-10, "volume {volume} against the recorded {printed}");
    assert_eq!(parts[1].faces.len(), 50);
}
