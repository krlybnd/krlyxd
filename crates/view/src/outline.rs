//! The `silhouette` query: where the model covers the image, row by row, as data; and,
//! given the silhouette of a photograph, how well the two agree.
//!
//! The file is named after the shape it reads, as `picture.rs` is; `silhouette` is the
//! query's name on the protocol.

use super::{image_params, Shot};
use krly3d_core::text::num;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;

pub struct Silhouette;

/// Rows reported as the most off when a reference is given.
const WORST: usize = 6;

impl Plugin<Query> for Silhouette {
    fn id(&self) -> Id {
        Id::new("silhouette")
    }

    fn schema(&self) -> Option<Schema> {
        let mut params = image_params();
        params.push(Param::alternative(
            "against",
            Kind::Any,
            "the silhouette to compare with, as [row, from, to] spans (columns from inclusive, to exclusive), such as krlyvis reads from a photograph taken by the same camera; only the rows from its first to its last are compared",
        ));
        Some(Schema {
            about: "where the drawn model covers the image, row by row, as [row, from, to] spans; with `against`, the overlap with another silhouette (IoU) and the rows where the edges differ most",
            inputs: (0, 0),
            params,
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        if let Err(error) = cx.check_control() {
            return Attempt::Failed(error);
        }
        let against = match request.params.get("against").map(|value| spans_of(cx, value)).transpose() {
            Ok(against) => against,
            Err(error) => return Attempt::Failed(error),
        };
        let result = match Shot::take(cx, request) {
            Attempt::Done(shot) => outlined(cx, &shot, against.as_ref()).into(),
            Attempt::Declined(reason) => Attempt::Declined(reason),
            Attempt::Failed(error) => Attempt::Failed(error),
        };
        cx.publish_control(|| result).unwrap_or_else(Attempt::Failed)
    }
}

/// Covered columns per row, as sorted half-open spans.
type Spans = BTreeMap<usize, Vec<(usize, usize)>>;

fn spans_of(cx: &Context, value: &Value) -> Result<Spans, Error> {
    let Value::List(items) = value else {
        return Err(Error::input("`against` is a list of [row, from, to] spans"));
    };
    let mut spans = Spans::new();
    for item in items {
        cx.check_control()?;
        let numbers: Option<Vec<usize>> = match item {
            Value::List(parts) if parts.len() == 3 => parts
                .iter()
                .map(|part| match part {
                    Value::Number(number) if *number >= 0.0 && number.fract() == 0.0 => {
                        Some(*number as usize)
                    }
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        match numbers.as_deref() {
            Some([row, from, to]) if from < to => spans.entry(*row).or_default().push((*from, *to)),
            _ => {
                return Err(Error::input(format!("`{item:?}` is not a [row, from, to] span with from < to")))
            }
        }
    }
    for row in spans.values_mut() {
        cx.check_control()?;
        row.sort_unstable();
    }
    Ok(spans)
}

fn covered(cx: &Context, shot: &Shot) -> Result<Spans, Error> {
    let (width, height) = (shot.image.width(), shot.image.height());
    let mut spans = Spans::new();
    for row in 0..height {
        cx.check_control()?;
        let mut column = 0;
        while column < width {
            if shot.image.seen[row * width + column].is_none() {
                column += 1;
                continue;
            }
            let from = column;
            while column < width && shot.image.seen[row * width + column].is_some() {
                column += 1;
            }
            spans.entry(row).or_default().push((from, column));
        }
    }
    Ok(spans)
}

fn count(row: &[(usize, usize)]) -> usize {
    row.iter().map(|(from, to)| to - from).sum()
}

/// Pixels covered by both rows of spans.
fn common(a: &[(usize, usize)], b: &[(usize, usize)]) -> usize {
    a.iter().flat_map(|(af, at)| b.iter().map(move |(bf, bt)| at.min(bt).saturating_sub(*af.max(bf)))).sum()
}

fn outlined(cx: &Context, shot: &Shot, against: Option<&Spans>) -> Result<Answer, Error> {
    let model = covered(cx, shot)?;
    let pixels: usize = model.values().map(|row| count(row)).sum();
    let rows = model.keys().next().zip(model.keys().last());
    let mut text = format!(
        "{} — {pixels} px{}\n",
        shot.headline("silhouette", 0).split(':').next().unwrap_or("silhouette"),
        rows.map_or_else(String::new, |(top, bottom)| format!(" in rows {top}..{bottom}"))
    );
    let mut data = shot.data();
    let mut listed = Vec::new();
    for (row, spans) in &model {
        cx.check_control()?;
        listed.extend(
            spans.iter().map(|(from, to)| Value::List(vec![(*row).into(), (*from).into(), (*to).into()])),
        );
    }
    data.insert("spans".into(), Value::List(listed));
    data.insert("pixels".into(), Value::from(pixels));
    if let Some(reference) = against {
        // The reference says which part of the image it covers: rows outside it are not compared.
        let first = reference.keys().next().copied().unwrap_or(0);
        let last = reference.keys().last().copied().unwrap_or(0);
        let all_rows: std::collections::BTreeSet<usize> = model
            .keys()
            .chain(reference.keys())
            .copied()
            .filter(|row| (first..=last).contains(row))
            .collect();
        let empty = Vec::new();
        let (mut both, mut either) = (0, 0);
        let mut off: Vec<(usize, i64, i64)> = Vec::new();
        for row in &all_rows {
            cx.check_control()?;
            let (mine, theirs) = (model.get(row).unwrap_or(&empty), reference.get(row).unwrap_or(&empty));
            let shared = common(mine, theirs);
            both += shared;
            either += count(mine) + count(theirs) - shared;
            let edges = |spans: &[(usize, usize)]| {
                spans.first().zip(spans.last()).map(|(first, last)| (first.0 as i64, last.1 as i64))
            };
            match (edges(mine), edges(theirs)) {
                (Some((ml, mr)), Some((tl, tr))) => off.push((*row, ml - tl, mr - tr)),
                (Some((ml, mr)), None) => off.push((*row, -(mr - ml), mr - ml)),
                (None, Some((tl, tr))) => off.push((*row, tr - tl, -(tr - tl))),
                (None, None) => {}
            }
        }
        let iou = if either == 0 { 1.0 } else { both as f64 / either as f64 };
        off.sort_by_key(|(_, left, right)| std::cmp::Reverse(left.abs().max(right.abs())));
        text.push_str(&format!("against the reference: IoU {}\n", num(iou)));
        text.push_str("most off (model − reference, px; left > 0: the model's left edge lies right of the reference's):\n");
        for (row, left, right) in off.iter().take(WORST) {
            text.push_str(&format!("row {row}: left {left:+}, right {right:+}\n"));
        }
        data.insert("iou".into(), Value::from(iou));
        let worst: Vec<Value> = off
            .iter()
            .take(WORST)
            .map(|(row, left, right)| {
                Value::List(vec![(*row).into(), (*left as f64).into(), (*right as f64).into()])
            })
            .collect();
        data.insert("worst".into(), Value::List(worst));
    }
    let answer = Answer { text, data: Value::Map(data), exact: shot.exactness() };
    cx.publish_control(|| answer)
}

#[cfg(test)]
mod tests {
    use crate::testing::session;
    use krly3d_kernel::schema::{Params, Value};

    fn entry<'a>(data: &'a Value, name: &str) -> &'a Value {
        let Value::Map(entries) = data else { panic!("not a map") };
        &entries[name]
    }

    #[test]
    fn a_silhouette_agrees_with_itself_and_reports_a_shifted_copy_by_its_edges() {
        let mut session = session();
        let small = Params::new().with("width", 120.0).with("height", 90.0);
        let own = session.call("silhouette", &small);
        let Value::List(spans) = entry(&own.data, "spans").clone() else { panic!("no spans") };
        let shifted: Vec<Value> = spans
            .iter()
            .map(|span| {
                let Value::List(parts) = span else { panic!("not a span") };
                let number =
                    |at: usize| if let Value::Number(n) = parts[at] { n } else { panic!("not a number") };
                Value::List(vec![number(0).into(), (number(1) + 3.0).into(), (number(2) + 3.0).into()])
            })
            .collect();

        let same = session.call("silhouette", &small.clone().with("against", Value::List(spans)));
        let moved = session.call("silhouette", &small.with("against", Value::List(shifted)));

        assert_eq!(entry(&same.data, "iou"), &Value::from(1.0));
        assert!(matches!(entry(&moved.data, "iou"), Value::Number(iou) if *iou < 0.95), "{}", moved.text);
        assert!(moved.text.contains(": left -3, right -3"), "{}", moved.text);
    }

    #[test]
    fn rows_outside_the_reference_are_not_compared() {
        let mut session = session();
        let small = Params::new().with("width", 120.0).with("height", 90.0);
        let own = session.call("silhouette", &small);
        let Value::List(spans) = entry(&own.data, "spans").clone() else { panic!("no spans") };
        // The upper half of the model's own silhouette, as a photograph cut off at a row.
        let cut: Vec<Value> = spans
            .into_iter()
            .filter(|span| matches!(span, Value::List(parts) if matches!(parts[0], Value::Number(row) if row < 45.0)))
            .collect();

        let compared = session.call("silhouette", &small.with("against", Value::List(cut)));

        assert_eq!(entry(&compared.data, "iou"), &Value::from(1.0), "{}", compared.text);
    }
}
