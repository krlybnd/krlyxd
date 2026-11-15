//! Run in release mode; times include mesh lookup/conversion and rasterization, not PNG.
use krly3d_core::math::Vec3;
use krly3d_kernel::schema::Params;
use krly3d_view::ports::render::{render, Frame, RenderRequest, View};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: preview_timing MODEL.k3d NODE [TOLERANCE]")?;
    let node = args.next().ok_or("missing node")?;
    let tolerance: f64 = args.next().map_or(Ok(0.05), |value| value.parse())?;
    let text = std::fs::read_to_string(&path)?;
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    let mut session = builder.session();
    let opened = session.call("open", &Params::new().with("text", text));
    if !opened.ok {
        return Err(opened.text.into());
    }
    let body = session.kernel().body(&node)?;
    let request = RenderRequest {
        bodies: vec![body],
        view: View { toward: Vec3::new(-1.0, 1.0, -1.0), up: Vec3::z(), frame: Frame::Fit },
        size: (256, 192),
        paint: vec![],
    };
    let cx = session.kernel().context();
    let start = Instant::now();
    let exact = render(cx, "raycast", &request).into_result()?;
    let exact_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let cold = krly3d_view_mesh::render_mesh(cx, &request, tolerance).into_result()?;
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let warm = krly3d_view_mesh::render_mesh(cx, &request, tolerance).into_result()?;
    let warm_ms = start.elapsed().as_secs_f64() * 1000.0;
    if cold != warm {
        return Err("cold and warm images differ".into());
    }
    let (intersection, union) =
        exact.seen.iter().zip(&warm.seen).fold((0_u64, 0_u64), |(both, either), (a, b)| {
            (both + u64::from(a.is_some() && b.is_some()), either + u64::from(a.is_some() || b.is_some()))
        });
    let iou = if union == 0 { 1.0 } else { intersection as f64 / union as f64 };
    println!("model={path} node={node} size=256x192 tolerance={tolerance} model-units profile={} exact_ms={exact_ms:.3} cold_mesh_ms={cold_ms:.3} warm_mesh_ms={warm_ms:.3} silhouette_iou={iou:.6}", if cfg!(debug_assertions) { "debug" } else { "release" });
    Ok(())
}
