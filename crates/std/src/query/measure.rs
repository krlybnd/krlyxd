//! `measure`: the numbers of a node on one line.

use super::node;
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;

pub struct Measure;

/// The measures that state a size, and so how exact they are, in the order they are read.
const SIZES: [&str; 2] = ["volume", "area"];
/// The measures that count.
const COUNTS: [&str; 3] = ["faces", "edges", "vertices"];

impl Plugin<Query> for Measure {
    fn id(&self) -> Id {
        Id::new("measure")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "volume, area and the numbers of faces, edges and vertices of a node; `=` marks an exact size, `~` a numerical one, `?` a sampled one; a sheet has no volume",
            inputs: (0, 0),
            params: vec![node()],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        measured(cx, request).into()
    }
}

fn measured(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let mut words = Vec::new();
    let mut data: BTreeMap<String, Value> = BTreeMap::new();
    let mut symbols: BTreeMap<String, Value> = BTreeMap::new();
    let mut exact = Exactness::Exact;
    for name in SIZES.into_iter().filter(|name| body.solid || *name != "volume") {
        let size = measure(cx, name, body).into_result()?;
        words.push(format!("{name} {}{}", size.exact.symbol(), text::num(size.value)));
        data.insert(name.to_string(), Value::from(size.value));
        symbols.insert(name.to_string(), Value::from(size.exact.symbol().to_string()));
        exact = exact.and(size.exact);
    }
    for name in COUNTS {
        let count = measure(cx, name, body).into_result()?;
        words.push(format!("{name} {}", text::num(count.value)));
        data.insert(name.to_string(), Value::from(count.value));
    }
    data.insert("exact".to_string(), Value::Map(symbols));
    Ok(Answer { text: words.join(" "), data: Value::Map(data), exact })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, number, part, plate_with_hole, word};
    use krly3d_core::Exactness;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_block_reads_its_volume_area_and_counts_on_one_line() {
        let answer = asked(&block(), "measure", part());

        assert_eq!(answer.text, "volume =8000 area =2800 faces 6 edges 12 vertices 8");
        assert_eq!(answer.exact, Exactness::Exact);
    }

    #[test]
    fn a_sheet_has_an_area_and_no_volume() {
        let answer = asked(&plate_with_hole(), "measure", Params::new().with("node", "outline"));

        assert_eq!(answer.text, "area =2349.734518 faces 1 edges 5 vertices 4");
    }

    #[test]
    fn the_numbers_come_as_data_in_full_precision_with_how_exact_they_are() {
        let answer = asked(&plate_with_hole(), "measure", part());

        let volume = number(entry(&answer.data, "volume"));
        assert!((volume - (24_000.0 - 160.0 * PI)).abs() < 1e-9, "{volume}");
        assert_eq!(number(entry(&answer.data, "faces")), 7.0);
        assert_eq!(word(entry(entry(&answer.data, "exact"), "volume")), "=");
    }
}
