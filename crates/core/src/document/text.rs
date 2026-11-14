//! The native text format: reading and writing a document.
//!
//! ```text
//! k3d 1
//! layer geo
//! plate = box w=60 d=40 h=10
//! hole  = cylinder r=4 h=10 at=20,15,0
//!   expect faces=3
//! layer ai
//! plate#top note="datum face"
//! ```
//!
//! One line is one statement. In a layer of statements a line is `name = tool`, followed
//! by the nodes the tool reads and by `key=value` parameters, in any order. The reader
//! knows no tool by name. Lines of other layers are kept as written; which layers hold
//! statements is said by the kernel that reads (`geo` for krly3d, `vis` for krlyvis).

use super::{Document, Layer, NodeRef, Statement};
use crate::schema::{Params, Value};
use crate::Error;
use std::fmt::Write as _;

/// The document written in `text`, whose layers named in `statements` hold statements.
pub fn parse(text: &str, statements: &[&str]) -> Result<Document, Error> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'));
    match lines.next() {
        Some((_, "k3d 1")) => {}
        Some((number, other)) => {
            return Err(Error::format(format!("line {number}: expected `k3d 1`, got `{other}`")))
        }
        None => return Err(Error::format("the text is empty; a k3d file starts with `k3d 1`")),
    }
    let mut document = Document::default();
    for (number, line) in lines {
        let at = |message: String| Error::format(format!("line {number}: {message}"));
        let words = words(line).map_err(at)?;
        if let ["layer", name] = words.as_slice() {
            document.layers.push(Layer { name: name.to_string(), ..Layer::default() });
            continue;
        }
        let layer = document.layers.last_mut().ok_or_else(|| at("a `layer` line must come first".into()))?;
        if !statements.contains(&layer.name.as_str()) {
            layer.lines.push(line.to_string());
        } else if let Some(expectation) = line.strip_prefix("expect ") {
            let statement = layer
                .statements
                .last_mut()
                .ok_or_else(|| at("`expect` has no statement before it".into()))?;
            statement.expect.push(expectation.trim().to_string());
        } else {
            layer.statements.push(statement(&words).map_err(|error| at(error.message))?);
        }
    }
    Ok(document)
}

/// Splits a line at spaces, keeping a double-quoted stretch together.
fn words(line: &str) -> Result<Vec<&str>, String> {
    let mut words = Vec::new();
    let (mut start, mut quoted) = (None, false);
    for (at, character) in line.char_indices() {
        match character {
            '"' => {
                quoted = !quoted;
                start = start.or(Some(at));
            }
            space if space.is_whitespace() && !quoted => {
                words.extend(start.take().map(|start| &line[start..at]));
            }
            _ => start = start.or(Some(at)),
        }
    }
    if quoted {
        return Err("a double quote is not closed".into());
    }
    words.extend(start.map(|start| &line[start..]));
    Ok(words)
}

fn statement(words: &[&str]) -> Result<Statement, Error> {
    let [name, "=", tool, rest @ ..] = words else {
        return Err(Error::format("expected `name = tool …`"));
    };
    let mut statement = Statement::new(name, tool, Params::new(), &[])?;
    for word in rest {
        match word.split_once('=') {
            Some((key, text)) if !key.is_empty() && !key.contains('"') => {
                statement.op.params.set(key, value(text))
            }
            _ => statement.op.inputs.push(NodeRef::parse(word)?),
        }
    }
    Ok(statement)
}

/// A quoted value is a text; a value with commas is a list; anything else is a number
/// when it reads as one and a text otherwise. A trailing comma makes a list of one.
fn value(text: &str) -> Value {
    if let Some(quoted) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Value::Text(quoted.to_string());
    }
    if text.contains(',') {
        return Value::List(text.split(',').filter(|item| !item.is_empty()).map(scalar).collect());
    }
    scalar(text)
}

fn scalar(text: &str) -> Value {
    let numeric = text.starts_with(|first: char| first.is_ascii_digit() || matches!(first, '-' | '+' | '.'));
    match (text, text.parse::<f64>()) {
        ("true", _) => Value::Bool(true),
        ("false", _) => Value::Bool(false),
        (_, Ok(number)) if numeric && number.is_finite() => Value::Number(number),
        _ => Value::Text(text.to_string()),
    }
}

/// The document as text.
pub fn print(document: &Document) -> Result<String, Error> {
    let mut text = String::from("k3d 1\n");
    for layer in &document.layers {
        let _ = writeln!(text, "layer {}", layer.name);
        for statement in &layer.statements {
            let _ = write!(text, "{} = {}", statement.name, statement.op.tool);
            for input in &statement.op.inputs {
                let _ = write!(text, " {input}");
            }
            for (name, value) in statement.op.params.iter() {
                let _ = write!(
                    text,
                    " {name}={}",
                    written(value).map_err(|problem| unwritable(&statement.name, name, problem))?
                );
            }
            text.push('\n');
            for expectation in &statement.expect {
                let _ = writeln!(text, "  expect {expectation}");
            }
        }
        for line in &layer.lines {
            let _ = writeln!(text, "{line}");
        }
    }
    Ok(text)
}

fn unwritable(node: &str, parameter: &str, problem: &str) -> Error {
    Error::format(format!("{node}: parameter `{parameter}` cannot be written: {problem}"))
}

fn written(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Map(_) => Err("a nested value has no place on one line"),
        Value::Number(number) if number.is_finite() => Ok(number.to_string()),
        Value::Number(_) => Err("it is not a finite number"),
        Value::Text(text) if text.contains('"') => Err("a text with a double quote in it"),
        Value::Text(text) if plain(text) => Ok(text.clone()),
        Value::Text(text) => Ok(format!("\"{text}\"")),
        Value::List(items) => {
            let mut text = String::new();
            for item in items {
                match item {
                    Value::Number(number) if number.is_finite() => text.push_str(&number.to_string()),
                    Value::Text(word) if plain(word) => text.push_str(word),
                    _ => return Err("a list may hold only numbers and plain words"),
                }
                text.push(',');
            }
            // The comma after the last item is dropped unless it is what marks the list.
            if items.len() > 1 {
                text.pop();
            } else if items.is_empty() {
                text.push(',');
            }
            Ok(text)
        }
    }
}

/// A text that reads back as the same text without quotes.
fn plain(text: &str) -> bool {
    let reads_as_number =
        text.starts_with(|first: char| first.is_ascii_digit() || matches!(first, '-' | '+' | '.'));
    !text.is_empty()
        && !matches!(text, "true" | "false")
        && !reads_as_number
        && !text.contains(|c: char| c.is_whitespace() || matches!(c, ',' | '=' | '"'))
}

#[cfg(test)]
mod tests {
    use super::{parse, print};

    const WRITTEN: &str = "k3d 1\nlayer vis\nphoto = picture path=\"refs/a b.jpg\"\ntop = outline photo region=0,0,400,300\n  expect points>=4\nlayer ai\nphoto note=\"front\"\n";

    #[test]
    fn a_document_reads_back_as_it_was_written() {
        let document = parse(WRITTEN, &["vis"]).unwrap();

        assert_eq!(print(&document).unwrap(), WRITTEN);
    }

    #[test]
    fn only_the_layers_named_hold_statements() {
        let document = parse(WRITTEN, &["vis"]).unwrap();

        let counts: Vec<(usize, usize)> =
            document.layers.iter().map(|layer| (layer.statements.len(), layer.lines.len())).collect();
        assert_eq!(counts, [(2, 0), (0, 1)]);
        assert_eq!(document.layers[0].statements[1].expect, ["points>=4"]);
    }

    #[test]
    fn a_text_without_its_header_is_refused_with_the_line() {
        let refused = parse("layer geo\n", &["geo"]).unwrap_err();

        assert!(refused.message.contains("line 1"));
    }
}
