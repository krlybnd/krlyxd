//! Station comparison over the pinned shape and services of a B-rep snapshot.
//! All projection, fill, contour intersections, verdicts and work limits belong to
//! the existing view station provider. No peer call or JSON conversion occurs here.

use krly3d_brep_engine::Snapshot;
use krly3d_core::{CallControl, Error};
use krly3d_kernel::model::NodeRef;
use krly3d_kernel::ports::likeness::report::{
    DrawingEvidence, DrawingFrame, StairPolicy, StationLikeness, StationOptions,
    StationPrecision as KernelPrecision, StationRequest,
};
use krly3d_kernel::ports::likeness::{Drawing, LikenessRequest};
use krly3d_kernel::{Context, Plugin};
use krly3d_view_api::likeness::{
    ComparisonScope, LikenessCall, LikenessProvenance, LikenessResult, Station, StationDifference,
    StationFrame, StationGrid, StationPolicy, StationPrecision, ENGINE_REVISION, REPORT_VERSION,
};
use krly3d_view_api::{diagnostic, Diagnostic, Exactness, Id, Length, LengthUnit, Quality};
use std::sync::Arc;

/// Compare one immutable snapshot in XY with pixel-derived contour evidence.
/// An unversioned node means this snapshot's version, never a new model lookup.
/// The snapshot and its services remain sufficient after the owning session changes
/// or is dropped. A differing node or explicit version is rejected before comparison.
pub fn compare_snapshot(
    snapshot: Snapshot,
    mut call: LikenessCall,
    control: Arc<CallControl>,
) -> Result<LikenessResult, Diagnostic> {
    let cx = snapshot.legacy_context().with_call_control(control);
    cx.check_control().map_err(legacy_diagnostic)?;
    call.validate()?;
    let requested = NodeRef::parse(&call.node).map_err(legacy_diagnostic)?;
    let pinned = NodeRef::parse(&snapshot.identity().node).map_err(legacy_diagnostic)?;
    if requested.name != pinned.name
        || requested.version.is_some_and(|version| Some(version) != pinned.version)
    {
        return Err(diagnostic("input", "likeness node/version does not match the supplied snapshot"));
    }
    call.node = snapshot.identity().node.clone();
    let request = StationRequest {
        comparison: LikenessRequest {
            shapes: vec![Arc::clone(snapshot.legacy_shape())],
            drawing: Drawing {
                axes: [0, 1],
                unit: call.evidence.pixel_unit,
                polygons: call.evidence.polygons.clone(),
            },
        },
        options: StationOptions {
            frame: DrawingFrame::default(),
            precision: KernelPrecision::Contour,
            tolerance: Some(call.evidence.pixel_unit),
            stairs: StairPolicy::None,
        },
        evidence: DrawingEvidence {
            vector: false,
            source: Some(call.evidence.source.clone()),
            uncertainty: None,
        },
    };
    // Standard B-rep snapshots intentionally have no view plugins registered.
    // Invoke the same concrete StationLikeness provider used by the legacy facade,
    // retaining the source context for its mesh/geometry providers and caches.
    let report = <krly3d_view::likeness::Raster as Plugin<StationLikeness>>::call(
        &krly3d_view::likeness::Raster,
        &cx,
        &request,
    )
    .into_result()
    .map_err(legacy_diagnostic)?;
    cx.check_control().map_err(legacy_diagnostic)?;
    let mesh_tolerance_requested = Length { value: call.evidence.pixel_unit, unit: LengthUnit::Model };
    let result = LikenessResult {
        request_id: call.request_id,
        report_version: REPORT_VERSION.into(),
        iou: report.resemblance.iou,
        model_area: report.resemblance.model_area,
        drawing_area: report.resemblance.drawing_area,
        raster_worst: report.resemblance.worst.iter().map(station).collect(),
        stations: differences(&cx, &report.stations).map_err(legacy_diagnostic)?,
        worst: differences(&cx, &report.worst).map_err(legacy_diagnostic)?,
        compared: report.compared,
        failed: report.failed,
        matches: report.matches,
        frame: StationFrame {
            origin: report.options.frame.origin,
            u: report.options.frame.u,
            v: report.options.frame.v,
            world_axes: request.comparison.drawing.axes,
        },
        grid: StationGrid {
            low: report.grid_low,
            cell: report.cell,
            size: report.size,
            samples: report.samples,
        },
        policy: StationPolicy {
            precision: StationPrecision::Contour,
            tolerance: call.evidence.pixel_unit,
            stair_allowance: 0.0,
            raster_precision_floor: report.cell,
            scope: ComparisonScope::SampledOuterExtents,
        },
        quality: Quality {
            exactness: Exactness::Sampled { samples: report.samples },
            requested_tolerance: Some(mesh_tolerance_requested),
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: LikenessProvenance {
            source: snapshot.identity().clone(),
            node: call.node,
            evidence: call.evidence,
            engine_revision: Id::new(ENGINE_REVISION),
            mesh_tolerance_requested,
            mesh_achieved_error: None,
            drawing_uncertainty: None,
        },
    };
    cx.publish_control(|| result).map_err(legacy_diagnostic)
}

fn station(station: &krly3d_kernel::ports::likeness::Station) -> Station {
    Station {
        along: station.along,
        at: station.at,
        model: station.model,
        drawing: station.drawing,
        off: station.off(),
    }
}

fn differences(
    cx: &Context,
    differences: &[krly3d_kernel::ports::likeness::report::StationDifference],
) -> Result<Vec<StationDifference>, Error> {
    let mut result = Vec::with_capacity(differences.len());
    for (index, difference) in differences.iter().enumerate() {
        if index % 256 == 0 {
            cx.check_control()?;
        }
        result.push(StationDifference {
            id: format!("{}:{}", ["u", "v"][difference.station.along], difference.index),
            index: difference.index,
            station: station(&difference.station),
            delta: difference.delta,
            tolerance: difference.tolerance,
            matches: difference.matches,
        });
    }
    Ok(result)
}

fn legacy_diagnostic(error: Error) -> Diagnostic {
    diagnostic(error.kind.label(), error.message)
}
