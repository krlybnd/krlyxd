//! `brief`: the whole model in a few lines, for an agent that opens a session.

use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::model::{Built, Document, Layer, NodeRef, Statement, GEOMETRY};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::ports::write::write;
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;

pub struct Brief;

/// The format statements are quoted in.
const FORMAT: &str = "k3d";

impl Plugin<Query> for Brief {
    fn id(&self) -> Id {
        Id::new("brief")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the model in a few lines: every version of every node in the order it was made, as its statement with the number of faces and the volume (the area for a sheet), then the other layers of the document with their number of lines",
            inputs: (0, 0),
            params: Vec::new(),
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        brief(cx, request).into()
    }
}

fn brief(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let (mut lines, mut nodes, mut exact) = (Vec::new(), Vec::new(), Exactness::Exact);
    let mut versions: BTreeMap<&str, u32> = BTreeMap::new();
    for statement in request.document.statements() {
        let version = versions.entry(statement.name.as_str()).or_insert(0);
        *version += 1;
        let node = NodeRef { name: statement.name.clone(), version: Some(*version) };
        let built = request
            .model
            .built(&node)
            .ok_or_else(|| Error::not_found(format!("`{node}` is in the document but was not built")))?;
        let named = |error: Error| Error::new(error.kind, format!("{node}: {}", error.message));
        let (line, entry, size) = node_line(cx, &node, statement, built).map_err(named)?;
        lines.push(line);
        nodes.push(entry);
        exact = exact.and(size);
    }
    let others: Vec<&Layer> = request.document.layers.iter().filter(|layer| layer.name != GEOMETRY).collect();
    for layer in &others {
        let count = layer.lines.len();
        lines.push(format!("layer {}: {count} {}", layer.name, if count == 1 { "line" } else { "lines" }));
    }
    let layers = others.iter().map(|layer| {
        Value::map([("name", Value::from(layer.name.as_str())), ("lines", Value::from(layer.lines.len()))])
    });
    Ok(Answer {
        text: if lines.is_empty() { "the model is empty".to_string() } else { lines.join("\n") },
        data: Value::map([("nodes", Value::List(nodes)), ("layers", Value::List(layers.collect()))]),
        exact,
    })
}

/// One version of a node as a line of text and as data, with how exact its size is:
/// `plate@1 = box d=40 h=10 w=60 -> 6 faces, volume =24000`.
fn node_line(
    cx: &Context,
    node: &NodeRef,
    statement: &Statement,
    built: &Built,
) -> Result<(String, Value, Exactness), Error> {
    let stated = stated(cx, statement)?;
    let faces = built.body.faces.len();
    // What a solid holds is its volume; a sheet holds none and is sized by its area.
    let measured = if built.body.solid { "volume" } else { "area" };
    let size = measure(cx, measured, &built.body).into_result()?;
    let line = format!(
        "{node} = {stated} -> {faces} {}, {measured} {}{}",
        if faces == 1 { "face" } else { "faces" },
        size.exact.symbol(),
        text::num(size.value)
    );
    let entry = Value::map([
        ("node", Value::from(node.name.as_str())),
        ("version", Value::from(node.version.unwrap_or(1) as usize)),
        ("tool", Value::from(statement.op.tool.as_str())),
        ("statement", Value::from(stated)),
        ("faces", Value::from(faces)),
        (measured, Value::from(size.value)),
    ]);
    Ok((line, entry, size.exact))
}

/// The tool, inputs and parameters of a statement as the document format writes them.
/// The writer is asked, so that a statement reads here exactly as it does in a saved file.
fn stated(cx: &Context, statement: &Statement) -> Result<String, Error> {
    let alone = Statement { expect: Vec::new(), ..statement.clone() };
    let layer = Layer { name: GEOMETRY.to_string(), statements: vec![alone], lines: Vec::new() };
    let written = write(cx, FORMAT, &Document { layers: vec![layer] }).into_result()?;
    let lead = format!("{} = ", statement.name);
    written.lines().find_map(|line| line.strip_prefix(&lead)).map(str::to_string).ok_or_else(|| {
        Error::format(format!("the `{FORMAT}` writer did not write the statement of `{}`", statement.name))
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, entry, items, kernel, number, word};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::read::read;
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::Kernel;

    fn plate() -> Params {
        Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0)
    }

    fn opened(text: &str) -> Kernel {
        let mut builder = crate::standard();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        let mut kernel = builder.build();
        let document = read(kernel.context(), "k3d", text).into_result().unwrap();
        kernel.open(document).unwrap();
        kernel
    }

    #[test]
    fn a_node_reads_as_its_statement_with_its_faces_and_volume() {
        let model = kernel(vec![("plate", "box", plate(), &[])]);

        let answer = asked(&model, "brief", Params::new());

        assert_eq!(answer.text, "plate@1 = box d=40 h=10 w=60 -> 6 faces, volume =24000");
    }

    #[test]
    fn every_version_of_every_node_has_a_line_in_the_order_it_was_made() {
        let model = kernel(vec![
            ("plate", "box", plate(), &[]),
            ("peg", "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[]),
            ("plate", "transform", Params::new().with("move", [0.0, 0.0, 5.0]), &["plate"]),
        ]);

        let answer = asked(&model, "brief", Params::new());

        let lines: Vec<&str> = answer.text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("plate@1 = box "), "{}", answer.text);
        assert_eq!(lines[1], "peg@1 = cylinder h=6 r=3 -> 3 faces, volume =169.646003");
        assert_eq!(lines[2], "plate@2 = transform plate move=0,0,5 -> 6 faces, volume =24000");
    }

    #[test]
    fn a_sheet_is_sized_by_its_area() {
        let model =
            kernel(vec![("outline", "profile", Params::new().with("path", "M 0,0 L 4,0 L 4,3 Z"), &[])]);

        let answer = asked(&model, "brief", Params::new());

        assert_eq!(answer.text, "outline@1 = profile path=\"M 0,0 L 4,0 L 4,3 Z\" -> 1 face, area =6");
    }

    #[test]
    fn the_other_layers_are_named_with_their_number_of_lines() {
        let model = opened(
            "k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\n  expect faces=6\nlayer ai\nplate#top note=\"datum\"\nplate watch=\"volume>=1\"\nlayer pmi.sem\nplate#top flatness=0.05\n",
        );

        let answer = asked(&model, "brief", Params::new());

        assert_eq!(
            answer.text,
            "plate@1 = box d=40 h=10 w=60 -> 6 faces, volume =24000\nlayer ai: 2 lines\nlayer pmi.sem: 1 line"
        );
    }

    #[test]
    fn the_same_comes_as_data() {
        let model = kernel(vec![("plate", "box", plate(), &[])]);

        let answer = asked(&model, "brief", Params::new());

        let nodes = items(entry(&answer.data, "nodes"));
        assert_eq!(nodes.len(), 1);
        assert_eq!((word(entry(&nodes[0], "node")), word(entry(&nodes[0], "tool"))), ("plate", "box"));
        assert_eq!((number(entry(&nodes[0], "faces")), number(entry(&nodes[0], "volume"))), (6.0, 24_000.0));
        assert!(items(entry(&answer.data, "layers")).is_empty());
    }

    #[test]
    fn an_empty_model_says_that_it_is_empty() {
        let answer = asked(&kernel(Vec::new()), "brief", Params::new());

        assert_eq!(answer.text, "the model is empty");
    }
}
