//! Narrow JSON boundary from krlysci's typed `.typ` contract to the native kernel.
//! Checks measured facts and point probes; finite probes do not prove shape equality.

use krly3d_core::{Accuracy, Id};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::query::ask;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::Kernel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::process::ExitCode;

#[derive(Serialize, Deserialize)]
struct Contract {
    spec: Spec,
    plan: Option<Plan>,
    target: Option<Target>,
    geometry_verified: bool,
}

#[derive(Serialize, Deserialize)]
struct Spec {
    schema_version: u8,
    id: String,
    revision: u64,
    parameters: BTreeMap<String, Quantity>,
    requirements: BTreeMap<String, Json>,
    semantic_hash: String,
}

#[derive(Serialize, Deserialize)]
struct Quantity {
    value_si: f64,
    dimension: Dimension,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Dimension {
    Scalar,
    Length,
    Area,
    Volume,
    Angle,
    Count,
}

#[derive(Serialize, Deserialize)]
struct Plan {
    id: String,
    design_spec_hash: String,
    #[serde(default)]
    revision: u64,
    nodes: BTreeMap<String, PlanNode>,
    output: String,
}

#[derive(Serialize, Deserialize)]
struct PlanNode {
    operation: Operation,
    inputs: Vec<String>,
    output_kind: PlanKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PlanKind {
    Region2,
    Solid,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Disk,
    Rectangle,
    Annulus,
    Extrude,
    Union,
    Difference,
}

#[derive(Serialize, Deserialize)]
struct Target {
    id: String,
    design_spec_hash: String,
    #[serde(default)]
    revision: u64,
    checks: Vec<Check>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Check {
    VolumeBetween { id: String, min_si: f64, max_si: f64 },
    SpanBetween { id: String, axis: Axis, min_si: f64, max_si: f64 },
    MaterialAt { id: String, point_si: [f64; 3], expected: bool },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Axis {
    X,
    Y,
    Z,
}

#[path = "cad_check/cad_history.rs"]
mod cad_history;
#[path = "cad_check/cad_synth.rs"]
mod cad_synth;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "--accept"
                | "--accept-reviewed"
                | "--undo"
                | "--redo"
                | "--show"
                | "--checkout"
                | "--mesh-history"
        )
    }) {
        return cad_history::cli(&args);
    }
    let (synthesis, path, budget) = match args.as_slice() {
        [path] => (false, path.as_str(), 0),
        [flag, path] if flag == "--synthesize" => (true, path.as_str(), 2_000),
        [flag, path, budget] if flag == "--synthesize" => {
            let Ok(budget) = budget.parse::<usize>() else {
                eprintln!("candidate budget must be an integer");
                return ExitCode::from(2);
            };
            (true, path.as_str(), budget)
        }
        _ => {
            eprintln!("usage: krly3d-cad-check [--synthesize] <contract.json|-> [candidate-budget]");
            return ExitCode::from(2);
        }
    };
    if synthesis && !(1..=50_000).contains(&budget) {
        eprintln!("candidate budget must be between 1 and 50000");
        return ExitCode::from(2);
    }
    let text = if path == "-" {
        let mut text = String::new();
        io::stdin().take(262_145).read_to_string(&mut text).map(|_| text)
    } else {
        std::fs::read_to_string(path)
    };
    let result = text.map_err(|error| error.to_string()).and_then(|text| {
        if text.len() > 262_144 {
            return Err("CAD contract exceeds 256 KiB".into());
        }
        let contract: Contract = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        Ok(if synthesis { cad_synth::synthesize(&contract, budget) } else { run(&contract) })
    });
    match result {
        Ok(report) => {
            let passed = report["status"] == "tested";
            println!("{report}");
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(contract: &Contract) -> Json {
    let fail = |reason: String| json!({ "status": "unknown", "reason": reason, "geometry_verified": false });
    if contract.spec.schema_version != 1 || contract.geometry_verified {
        return fail("unsupported or inconsistent contract version".into());
    }
    if contract.spec.requirements.is_empty()
        || contract.spec.requirements.values().any(|value| value["holds"] != true)
    {
        return fail("scalar requirement is failed or missing".into());
    }
    let (Some(plan), Some(target)) = (&contract.plan, &contract.target) else {
        return fail("both a plan and a geometry target are required".into());
    };
    if plan.design_spec_hash != contract.spec.semantic_hash
        || target.design_spec_hash != contract.spec.semantic_hash
        || plan.revision != contract.spec.revision
        || target.revision != contract.spec.revision
    {
        return fail("plan or target has a stale specification binding".into());
    }
    if plan.nodes.is_empty()
        || plan.nodes.len() > 128
        || target.checks.is_empty()
        || target.checks.len() > 128
    {
        return fail("plan or target is outside the supported size".into());
    }
    let result = build(contract, plan).map(|kernel| verify(&kernel, &plan.output, &target.checks));
    match result {
        Ok(checks) => {
            let status = if checks.iter().any(|item| item["status"] == "fail") {
                "fail"
            } else if checks.iter().any(|item| item["status"] == "unknown") {
                "unknown"
            } else {
                "tested"
            };
            json!({ "model_id": contract.spec.id, "revision": contract.spec.revision,
                "plan_id": plan.id, "target_id": target.id,
                "status": status, "checks": checks, "geometry_verified": false })
        }
        Err(reason) => fail(reason),
    }
}

fn preview_mesh(contract: &Contract) -> Result<Json, String> {
    if run(contract)["status"] != "tested" {
        return Err("accepted candidate no longer passes native checks".into());
    }
    let plan = contract.plan.as_ref().ok_or("accepted candidate has no plan")?;
    let kernel = build(contract, plan)?;
    let scale = contract
        .spec
        .parameters
        .values()
        .filter(|quantity| quantity.dimension == Dimension::Length)
        .map(|quantity| quantity.value_si.abs())
        .fold(1e-6_f64, f64::max);
    let tolerance = (scale * 0.005).max(kernel.context().accuracy().length() * 10.0);
    let mesh = query(
        &kernel,
        "mesh",
        Params::new().with("node", plan.output.as_str()).with("tolerance", tolerance).with("triangles", true),
    )?;
    let coordinates = match field(&mesh, "coordinates")? {
        Value::List(values) if values.len() <= 600_000 && values.len() % 3 == 0 => values,
        _ => return Err("mesh exceeds preview point limit".into()),
    };
    let corners = match field(&mesh, "corners")? {
        Value::List(values) if values.len() <= 1_200_000 && values.len() % 3 == 0 => values,
        _ => return Err("mesh exceeds preview triangle limit".into()),
    };
    let positions = coordinates.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    let indices = corners
        .iter()
        .map(|value| {
            let index = number(value)?;
            if index < 0.0 || index.fract() != 0.0 || index >= (positions.len() / 3) as f64 {
                return Err(String::from("mesh has an invalid triangle index"));
            }
            Ok(index as u32)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let volume = number(field(&mesh, "volume")?)?;
    let area = number(field(&mesh, "area")?)?;
    let open_edges = number(field(&mesh, "open")?)?;
    if open_edges < 0.0 || open_edges.fract() != 0.0 || open_edges > u32::MAX as f64 {
        return Err("mesh has an invalid open-edge count".into());
    }
    Ok(json!({ "positions": positions, "indices": indices, "volume": volume, "area": area,
        "open_edges": open_edges as u32, "source": plan.id, "units": "m" }))
}

fn build(contract: &Contract, plan: &Plan) -> Result<Kernel, String> {
    let scale = contract
        .spec
        .parameters
        .values()
        .filter(|quantity| quantity.dimension == Dimension::Length)
        .map(|quantity| quantity.value_si.abs())
        .fold(1e-6_f64, f64::max);
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(scale).map_err(|error| error.to_string())?);
    let mut kernel = builder.build();
    let mut ready = BTreeSet::new();
    let mut active = BTreeSet::new();
    make_node(&mut kernel, plan, &contract.spec, &plan.output, &mut ready, &mut active)?;
    if ready.len() != plan.nodes.len() {
        return Err("plan has unrelated nodes".into());
    }
    let body = kernel.body(&plan.output).map_err(|error| error.to_string())?;
    if !body.solid {
        return Err("plan output is not a solid".into());
    }
    Ok(kernel)
}

fn make_node(
    kernel: &mut Kernel,
    plan: &Plan,
    spec: &Spec,
    id: &str,
    ready: &mut BTreeSet<String>,
    active: &mut BTreeSet<String>,
) -> Result<(), String> {
    if ready.contains(id) {
        return Ok(());
    }
    if !active.insert(id.to_string()) {
        return Err(format!("cycle at node {id}"));
    }
    let node = plan.nodes.get(id).ok_or_else(|| format!("unknown node {id}"))?;
    let (tool, params, dependencies, expected_kind) = match node.operation {
        Operation::Disk => {
            let [r] = node.inputs.as_slice() else {
                return Err(format!("node {id}: disk arity"));
            };
            ("circle", Params::new().with("r", length(spec, r)?), Vec::new(), PlanKind::Region2)
        }
        Operation::Rectangle => {
            let [w, d] = node.inputs.as_slice() else {
                return Err(format!("node {id}: rectangle arity"));
            };
            (
                "rectangle",
                Params::new().with("w", length(spec, w)?).with("d", length(spec, d)?),
                Vec::new(),
                PlanKind::Region2,
            )
        }
        Operation::Annulus => {
            let [outer, inner] = node.inputs.as_slice() else {
                return Err(format!("node {id}: annulus arity"));
            };
            let (outer, inner) = (length(spec, outer)?, length(spec, inner)?);
            if outer <= inner {
                return Err(format!("node {id}: invalid annulus radii"));
            }
            let path = format!("O 0,0 {outer} O 0,0 {inner}");
            ("profile", Params::new().with("path", path), Vec::new(), PlanKind::Region2)
        }
        Operation::Extrude => {
            let [profile, height] = node.inputs.as_slice() else {
                return Err(format!("node {id}: extrude arity"));
            };
            (
                "extrude",
                Params::new().with("h", length(spec, height)?),
                vec![profile.clone()],
                PlanKind::Solid,
            )
        }
        Operation::Union | Operation::Difference => {
            let [left, right] = node.inputs.as_slice() else {
                return Err(format!("node {id}: boolean arity"));
            };
            (
                if matches!(node.operation, Operation::Union) { "union" } else { "subtract" },
                Params::new(),
                vec![left.clone(), right.clone()],
                PlanKind::Solid,
            )
        }
    };
    if node.output_kind != expected_kind {
        return Err(format!("node {id}: output kind mismatch"));
    }
    for dependency in &dependencies {
        make_node(kernel, plan, spec, dependency, ready, active)?;
    }
    let inputs: Vec<_> = dependencies.iter().map(String::as_str).collect();
    let statement = Statement::new(id, tool, params, &inputs).map_err(|error| error.to_string())?;
    kernel.apply(statement).map_err(|error| format!("node {id}: {error}"))?;
    active.remove(id);
    ready.insert(id.to_string());
    Ok(())
}

fn length(spec: &Spec, id: &str) -> Result<f64, String> {
    let quantity = spec.parameters.get(id).ok_or_else(|| format!("unknown parameter {id}"))?;
    if quantity.dimension != Dimension::Length || !quantity.value_si.is_finite() || quantity.value_si <= 0.0 {
        return Err(format!("parameter {id} must be a positive length"));
    }
    Ok(quantity.value_si)
}

fn verify(kernel: &Kernel, output: &str, checks: &[Check]) -> Vec<Json> {
    checks
        .iter()
        .map(|check| {
            let id = match check {
                Check::VolumeBetween { id, .. }
                | Check::SpanBetween { id, .. }
                | Check::MaterialAt { id, .. } => id,
            };
            let result = evaluate_check(kernel, output, check);
            match result {
                Ok((observed, passed)) => {
                    json!({ "id": id, "status": if passed { "pass" } else { "fail" }, "observed": observed })
                }
                Err(reason) => json!({ "id": id, "status": "unknown", "reason": reason }),
            }
        })
        .collect()
}

fn evaluate_check(kernel: &Kernel, output: &str, check: &Check) -> Result<(Json, bool), String> {
    match check {
        Check::VolumeBetween { min_si, max_si, .. } => {
            interval(*min_si, *max_si)?;
            let data = query(kernel, "measure", Params::new().with("node", output))?;
            let actual = number(field(&data, "volume")?)?;
            let eps = kernel.context().accuracy().volume();
            Ok((json!(actual), actual >= min_si - eps && actual <= max_si + eps))
        }
        Check::SpanBetween { axis, min_si, max_si, .. } => {
            interval(*min_si, *max_si)?;
            let data = query(kernel, "bounds", Params::new().with("node", output))?;
            let Value::List(size) = field(&data, "size")? else {
                return Err("bounds has no size triple".into());
            };
            let index = match axis {
                Axis::X => 0,
                Axis::Y => 1,
                Axis::Z => 2,
            };
            let actual = number(size.get(index).ok_or("incomplete bounds triple")?)?;
            let eps = kernel.context().accuracy().length();
            Ok((json!(actual), actual >= min_si - eps && actual <= max_si + eps))
        }
        Check::MaterialAt { point_si, expected, .. } => {
            if point_si.iter().any(|number| !number.is_finite()) {
                return Err("nonfinite sample point".into());
            }
            let data = query(
                kernel,
                "ray",
                Params::new()
                    .with("node", output)
                    .with("origin", *point_si)
                    .with("direction", [1.0, 0.0, 0.0]),
            )?;
            if let Value::List(hits) = field(&data, "hits")? {
                if let Some(first) = hits.first() {
                    let distance = number(field(first, "distance")?)?;
                    if distance <= kernel.context().accuracy().length() {
                        return Err("sample lies on or too near a boundary".into());
                    }
                }
            }
            let Value::Bool(actual) = field(&data, "inside")? else {
                return Err("ray has no inside flag".into());
            };
            Ok((json!(actual), actual == expected))
        }
    }
}

fn interval(min: f64, max: f64) -> Result<(), String> {
    if !min.is_finite() || !max.is_finite() || min < 0.0 || min > max {
        Err("invalid geometry interval".into())
    } else {
        Ok(())
    }
}

fn query(kernel: &Kernel, name: &str, params: Params) -> Result<Value, String> {
    let (document, model) = kernel.snapshot();
    ask(kernel.context(), &Id::new(name), &params, &document, &model)
        .into_result()
        .map(|answer| answer.data)
        .map_err(|error| error.to_string())
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value, String> {
    let Value::Map(map) = value else {
        return Err("query result is not an object".into());
    };
    map.get(name).ok_or_else(|| format!("query result has no {name}"))
}

fn number(value: &Value) -> Result<f64, String> {
    let Value::Number(number) = value else {
        return Err("query result is not a number".into());
    };
    if !number.is_finite() {
        return Err("query result is nonfinite".into());
    }
    Ok(*number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> Contract {
        let parameters = BTreeMap::from([
            ("w".into(), Quantity { value_si: 0.02, dimension: Dimension::Length }),
            ("d".into(), Quantity { value_si: 0.012, dimension: Dimension::Length }),
            ("h".into(), Quantity { value_si: 0.005, dimension: Dimension::Length }),
        ]);
        let nodes = BTreeMap::from([
            (
                "outline".into(),
                PlanNode {
                    operation: Operation::Rectangle,
                    inputs: vec!["w".into(), "d".into()],
                    output_kind: PlanKind::Region2,
                },
            ),
            (
                "body".into(),
                PlanNode {
                    operation: Operation::Extrude,
                    inputs: vec!["outline".into(), "h".into()],
                    output_kind: PlanKind::Solid,
                },
            ),
        ]);
        Contract {
            spec: Spec {
                schema_version: 1,
                id: "probe".into(),
                revision: 0,
                parameters,
                requirements: BTreeMap::from([("valid".into(), json!({"holds": true}))]),
                semantic_hash: "hash".into(),
            },
            plan: Some(Plan {
                id: "plan".into(),
                design_spec_hash: "hash".into(),
                revision: 0,
                nodes,
                output: "body".into(),
            }),
            target: Some(Target {
                id: "target".into(),
                design_spec_hash: "hash".into(),
                revision: 0,
                checks: vec![
                    Check::VolumeBetween { id: "volume".into(), min_si: 1.19e-6, max_si: 1.21e-6 },
                    Check::MaterialAt { id: "core".into(), point_si: [0.0, 0.0, 0.0025], expected: true },
                ],
            }),
            geometry_verified: false,
        }
    }

    #[test]
    fn native_measurement_and_material_probe_are_not_a_shape_proof() {
        let report = run(&block());
        assert_eq!(report["status"], "tested");
        assert_eq!(report["geometry_verified"], false);
        assert_eq!(report["checks"][0]["status"], "pass");
        assert_eq!(report["checks"][1]["status"], "pass");
    }

    #[test]
    fn wrong_target_fails_and_boundary_probe_remains_unknown() {
        let mut contract = block();
        if let Some(target) = &mut contract.target {
            target.checks[0] = Check::VolumeBetween { id: "volume".into(), min_si: 2e-6, max_si: 3e-6 };
        }
        assert_eq!(run(&contract)["status"], "fail");
        if let Some(target) = &mut contract.target {
            target.checks = vec![Check::MaterialAt {
                id: "boundary".into(),
                point_si: [0.01, 0.0, 0.0025],
                expected: true,
            }];
        }
        let result = run(&contract);
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["checks"][0]["status"], "unknown");
    }

    #[test]
    fn old_plan_is_stale_even_when_spec_hash_is_unchanged() {
        let mut contract = block();
        contract.spec.revision = 1;
        contract.target.as_mut().expect("target").revision = 1;
        let result = run(&contract);
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["reason"], "plan or target has a stale specification binding");
    }

    #[test]
    fn target_only_search_builds_composition_and_respects_budget() {
        let mut contract = block();
        contract.plan = None;
        contract.spec.parameters = BTreeMap::from([
            ("long".into(), Quantity { value_si: 0.02, dimension: Dimension::Length }),
            ("short".into(), Quantity { value_si: 0.005, dimension: Dimension::Length }),
            ("h".into(), Quantity { value_si: 0.004, dimension: Dimension::Length }),
        ]);
        let target = contract.target.as_mut().expect("target");
        target.checks = vec![
            Check::VolumeBetween { id: "volume".into(), min_si: 6.99e-7, max_si: 7.01e-7 },
            Check::SpanBetween { id: "x".into(), axis: Axis::X, min_si: 0.02, max_si: 0.02 },
            Check::SpanBetween { id: "y".into(), axis: Axis::Y, min_si: 0.02, max_si: 0.02 },
            Check::MaterialAt { id: "arm-x".into(), point_si: [0.009, 0.0, 0.002], expected: true },
            Check::MaterialAt { id: "arm-y".into(), point_si: [0.0, 0.009, 0.002], expected: true },
            Check::MaterialAt { id: "corner".into(), point_si: [0.009, 0.009, 0.002], expected: false },
        ];
        let report = cad_synth::synthesize(&contract, 2_000);
        assert_eq!(report["status"], "tested");
        assert_eq!(report["plan"]["nodes"]["body"]["operation"], "union");
        assert_eq!(report["cost_nodes"], 5);
        assert_eq!(report["geometry_verified"], false);

        contract.target.as_mut().expect("target").checks.push(Check::MaterialAt {
            id: "contradiction".into(),
            point_si: [0.009, 0.0, 0.002],
            expected: false,
        });
        let limited = cad_synth::synthesize(&contract, 1);
        assert_eq!(limited["status"], "unknown");
        assert_eq!(limited["reason"], "candidate budget exhausted");
        assert_eq!(limited["attempted"], 1);
    }

    #[test]
    fn accepted_history_replays_undo_redo_and_rejects_stale_revision() {
        let unique =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos();
        let history =
            std::env::temp_dir().join(format!("cad-history-test-{}-{unique}.json", std::process::id()));
        let source = history.with_extension("contract.json");
        std::fs::write(&source, serde_json::to_vec(&block()).expect("contract JSON")).expect("source");
        let first = cad_history::accept(&history, &source, 10).expect("first accepted revision");
        assert_eq!(first["revision"], 0);
        let mut revised = block();
        revised.spec.revision = 1;
        revised.plan.as_mut().expect("plan").revision = 1;
        revised.target.as_mut().expect("target").revision = 1;
        revised.target.as_mut().expect("target").checks[0] =
            Check::VolumeBetween { id: "volume".into(), min_si: 1.19e-6, max_si: 1.20e-6 };
        std::fs::write(&source, serde_json::to_vec(&revised).expect("revised JSON")).expect("source");
        assert!(cad_history::accept(&history, &source, 10)
            .unwrap_err()
            .contains("requires a validated review"));
        let initial = block();
        let review = json!({
            "before": initial.spec,
            "before_target": initial.target,
            "after": revised.spec,
            "after_target": revised.target,
            "revision": {
                "delta_id": "volume-change", "model_id": "probe",
                "base_revision": 0, "new_revision": 1,
                "base_hash": "hash", "result_hash": "hash"
            },
            "geometry_verified": false
        });
        let mut wrong_parent = review.clone();
        wrong_parent["before_target"]["id"] = json!("other-target");
        let rejected: Contract =
            serde_json::from_slice(&std::fs::read(&source).expect("source")).expect("candidate");
        assert!(cad_history::accept_inner(&history, rejected, 10, Some(wrong_parent))
            .unwrap_err()
            .contains("does not bind"));
        let second =
            cad_history::accept_inner(&history, revised, 10, Some(review)).expect("second accepted revision");
        assert_eq!(second["reviewed"], true);
        assert_eq!(second["revision"], 1);
        let saved = std::fs::read(&history).expect("saved history");
        let mut tampered: Json = serde_json::from_slice(&saved).expect("history JSON");
        tampered["entries"][1]["review"]["after_target"]["id"] = json!("other-target");
        std::fs::write(&history, serde_json::to_vec(&tampered).expect("tampered JSON"))
            .expect("tampered history");
        assert!(cad_history::move_cursor(&history, false).unwrap_err().contains("does not bind"));
        std::fs::write(&history, saved).expect("restore history");
        assert_eq!(cad_history::move_cursor(&history, false).expect("undo")["revision"], 0);
        assert_eq!(cad_history::move_cursor(&history, true).expect("redo")["revision"], 1);
        assert_eq!(cad_history::checkout(&history, 0).expect("checkout")["revision"], 0);
        assert_eq!(cad_history::checkout(&history, 1).expect("checkout")["revision"], 1);
        assert!(cad_history::accept(&history, &source, 10).unwrap_err().contains("stale"));
        std::fs::remove_file(&history).expect("remove history");
        std::fs::remove_file(&source).expect("remove source");
    }

    #[test]
    fn accepted_mesh_preview_is_native_and_bounded() {
        let mesh = preview_mesh(&block()).expect("native mesh preview");
        assert_eq!(mesh["open_edges"], 0);
        assert_eq!(mesh["units"], "m");
        assert_eq!(mesh["indices"].as_array().expect("indices").len(), 36);
        assert_eq!(mesh["positions"].as_array().expect("positions").len(), 24);
    }
}
