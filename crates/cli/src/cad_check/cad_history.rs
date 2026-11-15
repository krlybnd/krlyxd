//! Local accepted CAD revisions. Append-only branches keep prior accepted work.

use super::{cad_synth, preview_mesh, run, Contract, Json, Plan};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

#[derive(Serialize, Deserialize)]
struct History {
    schema_version: u8,
    model_id: String,
    active: usize,
    entries: Vec<Accepted>,
}

#[derive(Serialize, Deserialize)]
struct Accepted {
    parent: Option<usize>,
    contract: Contract,
    report: Json,
    #[serde(default)]
    review: Option<Json>,
}

pub(super) fn cli(args: &[String]) -> ExitCode {
    let outcome = match args {
        [command, history, contract] if command == "--accept" =>
            accept(Path::new(history), Path::new(contract), 2_000),
        [command, history, contract, budget] if command == "--accept" => {
            match budget.parse::<usize>() {
                Ok(value) if (1..=50_000).contains(&value) =>
                    accept(Path::new(history), Path::new(contract), value),
                _ => Err("candidate budget must be between 1 and 50000".into()),
            }
        }
        [command, history, review, validator] if command == "--accept-reviewed" => {
            accept_reviewed(Path::new(history), Path::new(review), Path::new(validator), 2_000)
        }
        [command, history, review, validator, budget] if command == "--accept-reviewed" => {
            match budget.parse::<usize>() {
                Ok(value) if (1..=50_000).contains(&value) => accept_reviewed(
                    Path::new(history), Path::new(review), Path::new(validator), value,
                ),
                _ => Err("candidate budget must be between 1 and 50000".into()),
            }
        }
        [command, history] if command == "--undo" => move_cursor(Path::new(history), false),
        [command, history] if command == "--redo" => move_cursor(Path::new(history), true),
        [command, history] if command == "--show" => show(Path::new(history)),
        [command, history] if command == "--mesh-history" => mesh_history(Path::new(history)),
        [command, history, index] if command == "--checkout" => index
            .parse::<usize>()
            .map_err(|_| "entry index must be an integer".to_string())
            .and_then(|index| checkout(Path::new(history), index)),
        _ => Err("usage: krly3d-cad-check --accept <history.json> <base-contract.json> [budget] | --accept-reviewed <history.json> <review-state.json> <krlysci-bin> [budget] | --undo|--redo|--show|--mesh-history <history.json> | --checkout <history.json> <entry-index>".into()),
    };
    match outcome {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn show(path: &Path) -> Result<Json, String> {
    let store = load(path)?.ok_or("history does not exist")?;
    let report = run(&store.entries[store.active].contract);
    if report["status"] != "tested" {
        return Err("active accepted revision no longer passes native checks".into());
    }
    Ok(summary(&store))
}

fn mesh_history(path: &Path) -> Result<Json, String> {
    let store = load(path)?.ok_or("history does not exist")?;
    preview_mesh(&store.entries[store.active].contract)
}

pub(super) fn accept(path: &Path, source: &Path, budget: usize) -> Result<Json, String> {
    let text = fs::read_to_string(source).map_err(|error| error.to_string())?;
    if text.len() > 262_144 {
        return Err("CAD contract exceeds 256 KiB".into());
    }
    let contract: Contract = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    accept_inner(path, contract, budget, None)
}

fn accept_reviewed(path: &Path, review_path: &Path, validator: &Path, budget: usize) -> Result<Json, String> {
    let text = fs::read_to_string(review_path).map_err(|error| error.to_string())?;
    if text.len() > 262_144 {
        return Err("CAD review state exceeds 256 KiB".into());
    }
    let review: Json = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    // The publication-as-code CLI owns the typed delta and hash validation.
    // This adapter checks that its validated result belongs to the active parent.
    let mut child = Command::new(validator)
        .arg("cad-candidate")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("review validator: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("review validator stdin unavailable")?
        .write_all(text.as_bytes())
        .map_err(|error| format!("review validator input: {error}"))?;
    let output = child.wait_with_output().map_err(|error| format!("review validator: {error}"))?;
    if !output.status.success() {
        return Err(format!("review validator rejected state: {}", String::from_utf8_lossy(&output.stderr)));
    }
    if output.stdout.len() > 262_144 {
        return Err("validated CAD candidate exceeds 256 KiB".into());
    }
    let contract: Contract = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("review validator output: {error}"))?;
    accept_inner(path, contract, budget, Some(review))
}

pub(super) fn accept_inner(
    path: &Path,
    mut contract: Contract,
    budget: usize,
    reviewed: Option<Json>,
) -> Result<Json, String> {
    let mut store = match load(path)? {
        Some(existing) => {
            let current = &existing.entries[existing.active].contract.spec;
            if contract.spec.id != existing.model_id
                || Some(contract.spec.revision) != current.revision.checked_add(1)
            {
                return Err("stale or cross-model accepted revision".into());
            }
            let review = reviewed.as_ref().ok_or("revised acceptance requires a validated review state")?;
            matches_review(&existing.entries[existing.active].contract, &contract, review)?;
            existing
        }
        None => {
            if contract.spec.revision != 0 {
                return Err("first accepted revision must be zero".into());
            }
            if reviewed.is_some() {
                return Err("first accepted revision has no parent to review".into());
            }
            History { schema_version: 1, model_id: contract.spec.id.clone(), active: 0, entries: Vec::new() }
        }
    };
    if store.entries.len() >= 128 {
        return Err("local history is full (128 accepted revisions)".into());
    }
    if contract.plan.is_none() {
        let search = cad_synth::synthesize(&contract, budget);
        if search["status"] != "tested" {
            return Err(format!("candidate search did not pass: {search}"));
        }
        contract.plan =
            Some(serde_json::from_value::<Plan>(search["plan"].clone()).map_err(|error| error.to_string())?);
    }
    let report = run(&contract);
    if report["status"] != "tested" {
        return Err(format!("candidate did not pass native checks: {report}"));
    }
    let parent = if store.entries.is_empty() { None } else { Some(store.active) };
    store.entries.push(Accepted { parent, contract, report, review: reviewed });
    store.active = store.entries.len() - 1;
    save(path, &store)?;
    Ok(summary(&store))
}

fn matches_review(previous: &Contract, candidate: &Contract, review: &Json) -> Result<(), String> {
    let before = serde_json::to_value(&previous.spec).map_err(|error| error.to_string())?;
    let before_target = serde_json::to_value(&previous.target).map_err(|error| error.to_string())?;
    let after = serde_json::to_value(&candidate.spec).map_err(|error| error.to_string())?;
    let after_target = serde_json::to_value(&candidate.target).map_err(|error| error.to_string())?;
    let record = &review["revision"];
    if review["before"] != before
        || review["before_target"] != before_target
        || review["after"] != after
        || review["after_target"] != after_target
        || review["geometry_verified"] != false
        || record["model_id"] != candidate.spec.id
        || record["base_revision"] != previous.spec.revision
        || record["new_revision"] != candidate.spec.revision
        || record["base_hash"] != previous.spec.semantic_hash
        || record["result_hash"] != candidate.spec.semantic_hash
        || record["delta_id"].as_str().is_none_or(str::is_empty)
    {
        return Err("review state does not bind the accepted parent and candidate".into());
    }
    if candidate.geometry_verified {
        return Err("review candidate cannot claim complete geometry verification".into());
    }
    Ok(())
}

pub(super) fn move_cursor(path: &Path, forward: bool) -> Result<Json, String> {
    let mut store = load(path)?.ok_or("history does not exist")?;
    let next = if forward {
        (0..store.entries.len())
            .rev()
            .find(|&index| store.entries[index].parent == Some(store.active))
            .ok_or("no later accepted revision")?
    } else {
        store.entries[store.active].parent.ok_or("no earlier accepted revision")?
    };
    let report = run(&store.entries[next].contract);
    if report["status"] != "tested" {
        return Err("target accepted revision no longer passes native checks".into());
    }
    store.active = next;
    save(path, &store)?;
    Ok(summary(&store))
}

pub(super) fn checkout(path: &Path, index: usize) -> Result<Json, String> {
    let mut store = load(path)?.ok_or("history does not exist")?;
    let entry = store.entries.get(index).ok_or("unknown accepted entry")?;
    if run(&entry.contract)["status"] != "tested" {
        return Err("target accepted revision no longer passes native checks".into());
    }
    store.active = index;
    save(path, &store)?;
    Ok(summary(&store))
}

fn summary(store: &History) -> Json {
    let entry = &store.entries[store.active];
    json!({ "model_id": store.model_id, "active_entry": store.active,
        "accepted_entries": store.entries.len(), "parent_entry": entry.parent,
        "entry_parents": store.entries.iter().map(|item| item.parent).collect::<Vec<_>>(),
        "revision": entry.contract.spec.revision,
        "spec_hash": entry.contract.spec.semantic_hash,
        "plan_id": entry.contract.plan.as_ref().map(|plan| &plan.id),
        "status": entry.report["status"], "reviewed": entry.review.is_some(), "geometry_verified": false })
}

fn load(path: &Path) -> Result<Option<History>, String> {
    let text = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if text.len() > 16 * 1024 * 1024 {
        return Err("local history exceeds 16 MiB".into());
    }
    let store: History = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if store.schema_version != 1
        || store.entries.is_empty()
        || store.entries.len() > 128
        || store.active >= store.entries.len()
        || store.entries.iter().enumerate().any(|(index, entry)| {
            entry.contract.spec.id != store.model_id
                || entry.report["status"] != "tested"
                || match entry.parent {
                    Some(parent) => {
                        parent >= index
                            || Some(entry.contract.spec.revision)
                                != store.entries[parent].contract.spec.revision.checked_add(1)
                    }
                    None => index != 0 || entry.contract.spec.revision != 0,
                }
        })
    {
        return Err("inconsistent local CAD history".into());
    }
    for entry in &store.entries {
        if let (Some(parent), Some(review)) = (entry.parent, &entry.review) {
            matches_review(&store.entries[parent].contract, &entry.contract, review)?;
        } else if entry.parent.is_none() && entry.review.is_some() {
            return Err("base accepted revision cannot contain a review".into());
        }
    }
    Ok(Some(store))
}

fn save(path: &Path, store: &History) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("local history exceeds 16 MiB".into());
    }
    let name = path.file_name().ok_or("history path needs a filename")?.to_string_lossy();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = path.with_file_name(format!(".{name}.tmp-{}-{nonce}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
