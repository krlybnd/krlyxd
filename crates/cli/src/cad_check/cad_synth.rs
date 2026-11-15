//! Bounded, object-agnostic enumeration over the executable CAD grammar.
//! Analytic bounds only reject impossible candidates; the native kernel decides success.

use super::{
    build, interval, json, verify, Axis, BTreeMap, Check, Contract, Dimension, Json, Operation, Plan,
    PlanKind, PlanNode, Target,
};

#[derive(Clone, PartialEq, Eq)]
enum Profile {
    Disk(String),
    Rectangle(String, String),
    Annulus(String, String),
}

struct Solid {
    profile: Profile,
    height: String,
    volume: f64,
    span: [f64; 3],
}

pub(super) fn synthesize(contract: &Contract, budget: usize) -> Json {
    let unknown = |reason: &str| {
        json!({ "status": "unknown", "reason": reason, "geometry_verified": false,
            "optimality_proven": false,
            "attempted": 0, "budget": budget })
    };
    if contract.spec.schema_version != 1 || contract.geometry_verified {
        return unknown("unsupported or inconsistent contract version");
    }
    if contract.plan.is_some() {
        return unknown("synthesis expects a target-only contract without a plan");
    }
    if contract.spec.requirements.is_empty()
        || contract.spec.requirements.values().any(|value| value["holds"] != true)
    {
        return unknown("scalar requirement is failed or missing");
    }
    let Some(target) = &contract.target else {
        return unknown("geometry target is required");
    };
    if target.design_spec_hash != contract.spec.semantic_hash
        || target.revision != contract.spec.revision
        || target.checks.is_empty()
        || target.checks.len() > 128
    {
        return unknown("invalid or stale geometry target");
    }
    for check in &target.checks {
        match check {
            Check::VolumeBetween { min_si, max_si, .. } | Check::SpanBetween { min_si, max_si, .. } => {
                if interval(*min_si, *max_si).is_err() {
                    return unknown("invalid target interval");
                }
            }
            Check::MaterialAt { point_si, .. } => {
                if point_si.iter().any(|value| !value.is_finite()) {
                    return unknown("invalid target point");
                }
            }
        }
    }
    let lengths: Vec<_> = contract
        .spec
        .parameters
        .iter()
        .filter(|(_, value)| {
            value.dimension == Dimension::Length && value.value_si.is_finite() && value.value_si > 0.0
        })
        .map(|(id, value)| (id.clone(), value.value_si))
        .collect();
    if lengths.is_empty() || lengths.len() > 6 {
        return unknown("bounded search supports 1..6 positive length parameters");
    }
    let solids = enumerate(&lengths);
    let mut singles: Vec<_> = solids
        .iter()
        .enumerate()
        .filter(|(_, solid)| possible(target, solid.volume, solid.volume, solid.span, true))
        .map(|(index, solid)| (score(target, solid.volume, solid.span), index))
        .collect();
    singles.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut attempted = 0;
    let mut native_unknown = 0;
    for (_, index) in singles {
        if attempted == budget {
            return exhausted(contract, budget, attempted, native_unknown, true);
        }
        let plan = make_plan(contract, &solids[index], None);
        if let Some(report) =
            try_candidate(contract, target, plan, budget, &mut attempted, &mut native_unknown)
        {
            return report;
        }
    }

    // Both Boolean operations are structural grammar rules, not object templates.
    // Necessary volume and span bounds prune pairs before expensive native builds.
    let mut pairs = Vec::new();
    for (left_index, left) in solids.iter().enumerate() {
        for (right_index, right) in solids.iter().enumerate() {
            if left_index == right_index {
                continue;
            }
            let union_span = [
                left.span[0].max(right.span[0]),
                left.span[1].max(right.span[1]),
                left.span[2].max(right.span[2]),
            ];
            if left_index < right_index
                && possible(
                    target,
                    left.volume.max(right.volume),
                    left.volume + right.volume,
                    union_span,
                    true,
                )
            {
                let estimated = (left.volume.max(right.volume) + left.volume + right.volume) / 2.0;
                pairs.push((score(target, estimated, union_span), left_index, right_index, Operation::Union));
            }
            if possible(target, (left.volume - right.volume).max(0.0), left.volume, left.span, false) {
                let estimated = (left.volume - right.volume).max(0.0);
                pairs.push((
                    score(target, estimated, left.span),
                    left_index,
                    right_index,
                    Operation::Difference,
                ));
            }
        }
    }
    pairs.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(matches!(a.3, Operation::Difference).cmp(&matches!(b.3, Operation::Difference)))
    });
    for (_, left, right, operation) in pairs {
        if attempted == budget {
            return exhausted(contract, budget, attempted, native_unknown, true);
        }
        let plan = make_plan(contract, &solids[left], Some((&solids[right], operation)));
        if let Some(report) =
            try_candidate(contract, target, plan, budget, &mut attempted, &mut native_unknown)
        {
            return report;
        }
    }
    exhausted(contract, budget, attempted, native_unknown, false)
}

fn enumerate(lengths: &[(String, f64)]) -> Vec<Solid> {
    let mut profiles = Vec::new();
    for (radius, r) in lengths {
        profiles.push((Profile::Disk(radius.clone()), std::f64::consts::PI * r * r, [2.0 * r, 2.0 * r]));
    }
    for (width, w) in lengths {
        for (depth, d) in lengths {
            profiles.push((Profile::Rectangle(width.clone(), depth.clone()), w * d, [*w, *d]));
        }
    }
    for (outer, outer_r) in lengths {
        for (inner, inner_r) in lengths {
            if outer_r > inner_r {
                profiles.push((
                    Profile::Annulus(outer.clone(), inner.clone()),
                    std::f64::consts::PI * (outer_r * outer_r - inner_r * inner_r),
                    [2.0 * outer_r, 2.0 * outer_r],
                ));
            }
        }
    }
    let mut solids = Vec::new();
    for (profile, area, xy) in profiles {
        for (height, h) in lengths {
            solids.push(Solid {
                profile: profile.clone(),
                height: height.clone(),
                volume: area * h,
                span: [xy[0], xy[1], *h],
            });
        }
    }
    solids
}

fn possible(target: &Target, volume_low: f64, volume_high: f64, span: [f64; 3], span_exact: bool) -> bool {
    target.checks.iter().all(|check| match check {
        Check::VolumeBetween { min_si, max_si, .. } => overlaps(volume_low, volume_high, *min_si, *max_si),
        Check::SpanBetween { axis, min_si, max_si, .. } => {
            let value = span[axis_index(*axis)];
            if span_exact {
                overlaps(value, value, *min_si, *max_si)
            } else {
                overlaps(0.0, value, *min_si, *max_si)
            }
        }
        Check::MaterialAt { .. } => true,
    })
}

fn overlaps(a: f64, b: f64, low: f64, high: f64) -> bool {
    let tolerance = 1e-9 * (a.abs() + b.abs() + low.abs() + high.abs()).max(1e-18);
    a <= high + tolerance && b + tolerance >= low
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

fn score(target: &Target, volume: f64, span: [f64; 3]) -> f64 {
    target
        .checks
        .iter()
        .map(|check| {
            let (value, min, max) = match check {
                Check::VolumeBetween { min_si, max_si, .. } => (volume, *min_si, *max_si),
                Check::SpanBetween { axis, min_si, max_si, .. } => {
                    (span[axis_index(*axis)], *min_si, *max_si)
                }
                Check::MaterialAt { .. } => return 0.0,
            };
            let gap = if value < min { min - value } else { (value - max).max(0.0) };
            gap / ((min + max) / 2.0).max(1e-18)
        })
        .sum()
}

fn make_plan(contract: &Contract, left: &Solid, right: Option<(&Solid, Operation)>) -> Plan {
    let mut nodes = BTreeMap::new();
    nodes.insert("region_a".into(), profile_node(&left.profile));
    nodes.insert(
        "solid_a".into(),
        PlanNode {
            operation: Operation::Extrude,
            inputs: vec!["region_a".into(), left.height.clone()],
            output_kind: PlanKind::Solid,
        },
    );
    let output = if let Some((other, operation)) = right {
        let region_id = if left.profile == other.profile {
            "region_a"
        } else {
            nodes.insert("region_b".into(), profile_node(&other.profile));
            "region_b"
        };
        nodes.insert(
            "solid_b".into(),
            PlanNode {
                operation: Operation::Extrude,
                inputs: vec![region_id.into(), other.height.clone()],
                output_kind: PlanKind::Solid,
            },
        );
        nodes.insert(
            "body".into(),
            PlanNode {
                operation,
                inputs: vec!["solid_a".into(), "solid_b".into()],
                output_kind: PlanKind::Solid,
            },
        );
        "body"
    } else {
        "solid_a"
    };
    Plan {
        id: "bounded-search".into(),
        design_spec_hash: contract.spec.semantic_hash.clone(),
        revision: contract.spec.revision,
        nodes,
        output: output.into(),
    }
}

fn profile_node(profile: &Profile) -> PlanNode {
    let (operation, inputs) = match profile {
        Profile::Disk(radius) => (Operation::Disk, vec![radius.clone()]),
        Profile::Rectangle(width, depth) => (Operation::Rectangle, vec![width.clone(), depth.clone()]),
        Profile::Annulus(outer, inner) => (Operation::Annulus, vec![outer.clone(), inner.clone()]),
    };
    PlanNode { operation, inputs, output_kind: PlanKind::Region2 }
}

fn try_candidate(
    contract: &Contract,
    target: &Target,
    plan: Plan,
    budget: usize,
    attempted: &mut usize,
    native_unknown: &mut usize,
) -> Option<Json> {
    *attempted += 1;
    match build(contract, &plan) {
        Ok(kernel) => {
            let checks = verify(&kernel, &plan.output, &target.checks);
            if checks.iter().all(|check| check["status"] == "pass") {
                let cost = plan.nodes.len();
                return Some(json!({ "model_id": contract.spec.id, "target_id": target.id,
                    "status": "tested", "geometry_verified": false, "optimality_proven": false,
                    "plan": plan,
                    "checks": checks, "cost_nodes": cost, "attempted": attempted,
                    "budget": budget }));
            }
            if checks.iter().any(|check| check["status"] == "unknown") {
                *native_unknown += 1;
            }
        }
        Err(_) => *native_unknown += 1,
    }
    None
}

fn exhausted(
    contract: &Contract,
    budget: usize,
    attempted: usize,
    native_unknown: usize,
    hit_budget: bool,
) -> Json {
    json!({ "model_id": contract.spec.id, "status": "unknown", "geometry_verified": false,
        "optimality_proven": false,
        "reason": if hit_budget { "candidate budget exhausted" } else { "bounded grammar exhausted" },
        "attempted": attempted, "native_unknown": native_unknown, "budget": budget })
}
