//! `mesh`: the node read as triangles, for whoever shows, prints or exports it, with what
//! is needed to trust the reading: how many triangles, the volume and area they hold and
//! whether they close.

use super::node;
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::interpret::mesh_of;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::ports::shape::Shape;
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::{Context, Plugin};
use krly3d_mesh::{
    ArtifactId, LengthUnit, MeshArtifact, MeshData, MeshInput, MeshResult, Provenance, Quality,
};
use krly3d_mesh::{DEFAULT_TOLERANCE, DEFAULT_TRIANGLES, MAX_SAFE_INTEGER};
use std::sync::Arc;

pub struct Mesh;

impl Plugin<Query> for Mesh {
    fn id(&self) -> Id {
        Id::new("mesh")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the node cut into triangles that stay within `tolerance` of it: their count, volume, area and the sides left open (none for a closed body)",
            inputs: (0, 0),
            params: vec![
                node(),
                Param::optional("tolerance", Kind::Number, DEFAULT_TOLERANCE, "how far the triangles may stray from the surface"),
                Param::optional("triangles", Kind::Flag, DEFAULT_TRIANGLES, "also give the points and the triangles themselves, for a client that draws or exports them"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        mesh(cx, request).into()
    }
}

fn mesh(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let shape = &request.node("node")?.shape;
    let mut input = MeshInput::new(request.params.text("node")?);
    input.tolerance.value = request.params.number("tolerance")?;
    input.triangles = request.params.flag("triangles")?;
    Ok(read(cx, shape, &input)?.answer())
}

/// One typed reading, before the session bridge attaches provenance and artifact identity.
/// Both the legacy query and the mesh engine use this calculation and `mesh_of` cache.
pub struct MeshReading {
    points: u64,
    triangles: u64,
    volume: f64,
    area: f64,
    open: u64,
    quality: Quality,
    data: Option<MeshData>,
}

impl MeshReading {
    pub fn into_result(self, provenance: Provenance, artifact_id: ArtifactId) -> MeshResult {
        MeshResult {
            points: self.points,
            triangles: self.triangles,
            volume: self.volume,
            area: self.area,
            open: self.open,
            quality: self.quality,
            provenance,
            mesh: self.data.map(|data| MeshArtifact { id: artifact_id, revision: 1, data }),
        }
    }

    fn answer(self) -> Answer {
        let mut data = Value::map([
            ("triangles", (self.triangles as f64).into()),
            ("points", (self.points as f64).into()),
            ("volume", self.volume.into()),
            ("area", self.area.into()),
            ("open", (self.open as f64).into()),
        ]);
        if let (Some(mesh), Value::Map(entries)) = (self.data, &mut data) {
            let points = mesh.points.into_iter().flatten().map(Value::from);
            let corners = mesh.triangles.into_iter().flatten().map(|index| Value::from(f64::from(index)));
            entries.insert("coordinates".to_string(), Value::List(points.collect()));
            entries.insert("corners".to_string(), Value::List(corners.collect()));
        }
        Answer {
            text: format!(
                "{} triangles, {} points, volume {}, area {}, {} open sides",
                self.triangles,
                self.points,
                text::num(self.volume),
                text::num(self.area),
                self.open,
            ),
            data,
            exact: self.quality.exactness,
        }
    }
}

/// The single calculation path. The API stays independent of this temporary bridge.
pub fn read(cx: &Context, shape: &Arc<dyn Shape>, input: &MeshInput) -> Result<MeshReading, Error> {
    cx.check_control()?;
    if input.tolerance.unit != LengthUnit::Model {
        return Err(Error::input("mesh tolerance only supports model units"));
    }
    let tolerance = input.tolerance.value;
    if !(tolerance.is_finite() && tolerance > cx.accuracy().length()) {
        return Err(Error::input(format!(
            "a mesh tolerance must be above the length tolerance, got {tolerance}"
        )));
    }
    let found = mesh_of(cx, shape, tolerance).into_result()?;
    let data = mesh_data(cx, &found, input.triangles)?;
    let (points, triangles, open) = (
        count(found.points.len())?,
        count(found.triangles.len())?,
        count(found.open_sides_checked(|| cx.check_control())?.len())?,
    );
    let (volume, area) =
        (found.volume_checked(|| cx.check_control())?, found.area_checked(|| cx.check_control())?);
    if !volume.is_finite() || !area.is_finite() {
        return Err(Error::geometry("a mesh reading must contain finite numbers"));
    }
    cx.check_control()?;
    Ok(MeshReading {
        points,
        triangles,
        volume,
        area,
        open,
        data,
        quality: Quality {
            exactness: Exactness::Sampled { samples: u32::try_from(triangles).unwrap_or(u32::MAX) },
            positional_uncertainty: None,
            confidence: None,
        },
    })
}

fn mesh_data(
    cx: &Context,
    found: &krly3d_kernel::mesh::Mesh,
    include: bool,
) -> Result<Option<MeshData>, Error> {
    cx.check_control()?;
    let mut data = include.then(|| MeshData {
        points: Vec::with_capacity(found.points.len()),
        triangles: Vec::with_capacity(found.triangles.len()),
    });
    for (index, point) in found.points.iter().enumerate() {
        if index % 256 == 0 {
            cx.check_control()?;
        }
        if !point.coords.iter().all(|value| value.is_finite()) {
            return Err(Error::geometry("a mesh reading must contain finite numbers"));
        }
        if let Some(data) = &mut data {
            data.points.push([point.x, point.y, point.z]);
        }
    }
    // Check before volume/area index the points, including for summary-only calls.
    for (ordinal, triangle) in found.triangles.iter().enumerate() {
        if ordinal % 256 == 0 {
            cx.check_control()?;
        }
        let corners = [index(triangle[0])?, index(triangle[1])?, index(triangle[2])?];
        if triangle.iter().any(|index| *index >= found.points.len()) {
            return Err(Error::geometry("a mesh triangle references a missing point"));
        }
        if let Some(data) = &mut data {
            data.triangles.push(corners);
        }
    }
    cx.check_control()?;
    Ok(data)
}

fn count(value: usize) -> Result<u64, Error> {
    u64::try_from(value)
        .ok()
        .filter(|count| *count <= MAX_SAFE_INTEGER)
        .ok_or_else(|| Error::unsupported("a mesh count exceeds the JSON-safe integer range"))
}

fn index(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::unsupported("a mesh index exceeds the u32 range"))
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, entry, kernel, part};
    use krly3d_kernel::schema::{Params, Value};
    use std::f64::consts::PI;

    fn number(value: &Value) -> f64 {
        match value {
            Value::Number(number) => *number,
            other => panic!("not a number: {other:?}"),
        }
    }

    #[test]
    fn a_ball_reads_as_closed_triangles_holding_its_volume() {
        let ball = kernel(vec![("part", "sphere", Params::new().with("r", 5.0), &[])]);

        let answer = asked(&ball, "mesh", part().with("tolerance", 0.01));

        assert_eq!(number(entry(&answer.data, "open")), 0.0);
        let exact = 4.0 / 3.0 * PI * 125.0;
        assert!((number(entry(&answer.data, "volume")) - exact).abs() < 0.01 * 4.0 * PI * 25.0);
    }

    #[test]
    fn indices_use_checked_u32_conversion() {
        assert_eq!(super::index(u32::MAX as usize).unwrap(), u32::MAX);
        if let Ok(too_big) = usize::try_from(u64::from(u32::MAX) + 1) {
            assert!(super::index(too_big).is_err());
        }
    }

    #[test]
    fn counts_never_round_through_the_legacy_number_encoding() {
        if let Ok(maximum) = usize::try_from(krly3d_mesh::MAX_SAFE_INTEGER) {
            assert_eq!(super::count(maximum).unwrap(), krly3d_mesh::MAX_SAFE_INTEGER);
            assert!(super::count(maximum + 1).is_err());
        }
    }

    #[test]
    fn invalid_indices_are_rejected_before_statistics_can_index_the_points() {
        let cx = krly3d_kernel::KernelBuilder::new().context();
        let mut mesh = krly3d_kernel::mesh::Mesh::default();
        mesh.triangles.push([0, 0, 0]);
        for include in [false, true] {
            assert_eq!(
                super::mesh_data(&cx, &mesh, include).unwrap_err().kind,
                krly3d_core::ErrorKind::Geometry
            );
        }
        if let Ok(too_big) = usize::try_from(u64::from(u32::MAX) + 1) {
            mesh.triangles[0] = [too_big; 3];
            for include in [false, true] {
                assert_eq!(
                    super::mesh_data(&cx, &mesh, include).unwrap_err().kind,
                    krly3d_core::ErrorKind::Unsupported
                );
            }
        }
    }

    #[test]
    fn artifact_copy_stops_between_point_chunks() {
        use krly3d_core::{CallControl, Clock, ErrorKind};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        struct StepClock(AtomicUsize);
        impl Clock for StepClock {
            fn now(&self) -> Duration {
                Duration::from_secs(self.0.fetch_add(1, Ordering::Relaxed) as u64)
            }
        }
        let clock = Arc::new(StepClock(AtomicUsize::new(0)));
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(2)));
        let cx = krly3d_kernel::KernelBuilder::new().context().with_call_control(control);
        let mut mesh = krly3d_kernel::mesh::Mesh {
            points: vec![krly3d_core::math::Point3::origin(); 1024],
            triangles: Vec::new(),
            ..krly3d_kernel::mesh::Mesh::default()
        };
        // If copying reaches the final chunk, invalid geometry wins instead of stopping.
        mesh.points[900].x = f64::NAN;
        assert_eq!(super::mesh_data(&cx, &mesh, true).unwrap_err().kind, ErrorKind::DeadlineExceeded);
        assert_eq!(clock.0.load(Ordering::Relaxed), 3);
    }
}
