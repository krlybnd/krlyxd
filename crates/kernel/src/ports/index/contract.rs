//! What every index must satisfy: one element per face, relations that hold both ways
//! between different elements, and scores that are the formula of the family, worked
//! out here from the keys and relations the index itself states.

use super::{Index, Search, ALPHA, HOPS};
use crate::topo::Body;
use krly3d_core::Accuracy;
use std::collections::BTreeSet;

pub fn check(index: &dyn Index, body: &Body, accuracy: &Accuracy) -> Result<(), String> {
    let names: Vec<&str> = body.faces.iter().map(|face| face.name.as_str()).collect();
    if index.elements() != names {
        return Err(format!("the elements {:?} are not the faces {names:?}", index.elements()));
    }
    for element in 0..names.len() {
        relations(index, element)?;
    }
    let keys: BTreeSet<&String> = (0..names.len()).flat_map(|element| index.keys(element)).collect();
    for key in keys {
        // Each key alone, as a key of the element and of its neighbours, and both at
        // once with different weights so that neither part can hide behind the other.
        let term = |weight: f64| vec![(key.clone(), weight)];
        for search in [
            Search { this: term(1.0), near: Vec::new() },
            Search { this: Vec::new(), near: term(1.0) },
            Search { this: term(2.0), near: term(3.0) },
        ] {
            scores(index, &search, accuracy)?;
        }
    }
    let unknown = Search { this: vec![("no such key".to_string(), 1.0)], near: Vec::new() };
    if index.score(&unknown).iter().any(|score| *score != 0.0) {
        return Err("a key that no element has scores above zero".to_string());
    }
    Ok(())
}

fn relations(index: &dyn Index, element: usize) -> Result<(), String> {
    let (elements, related) = (index.elements(), index.related(element));
    for (position, other) in related.iter().enumerate() {
        if *other == element || *other >= elements.len() {
            return Err(format!("`{}` is related to itself or to no element", elements[element]));
        }
        if related[..position].contains(other) {
            return Err(format!("`{}` is related to `{}` twice", elements[element], elements[*other]));
        }
        if !index.related(*other).contains(&element) {
            return Err(format!(
                "`{}` is related to `{}` but not the other way round",
                elements[element], elements[*other]
            ));
        }
    }
    Ok(())
}

fn scores(index: &dyn Index, search: &Search, accuracy: &Accuracy) -> Result<(), String> {
    let (stated, expected) = (index.score(search), by_the_formula(index, search));
    if stated.len() != expected.len() {
        return Err(format!("{} scores for {} elements", stated.len(), expected.len()));
    }
    for (element, (stated, expected)) in stated.iter().zip(&expected).enumerate() {
        if (stated - expected).abs() > accuracy.relative() * (1.0 + expected.abs()) {
            return Err(format!(
                "`{}` scores {stated} for {search:?}; the formula gives {expected}",
                index.elements()[element]
            ));
        }
    }
    Ok(())
}

/// `K·q + α·R·K·q_near + α²·R²·K·q_near`, element by element from the rows of K and R.
fn by_the_formula(index: &dyn Index, search: &Search) -> Vec<f64> {
    let count = index.elements().len();
    let matched = |terms: &[(String, f64)]| -> Vec<f64> {
        (0..count)
            .map(|element| {
                terms
                    .iter()
                    .filter(|(key, _)| index.keys(element).contains(key))
                    .map(|(_, weight)| weight)
                    .sum()
            })
            .collect()
    };
    let hop = |values: &[f64]| -> Vec<f64> {
        (0..count).map(|element| index.related(element).iter().map(|other| values[*other]).sum()).collect()
    };
    let mut score = matched(&search.this);
    let (mut passed, mut weight) = (matched(&search.near), 1.0);
    for _ in 0..HOPS {
        passed = hop(&passed);
        weight *= ALPHA;
        for (total, share) in score.iter_mut().zip(&passed) {
            *total += weight * share;
        }
    }
    score
}
