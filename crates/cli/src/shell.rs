//! The shell form of a call: `krly3d call box id=plate w=60 d=40 h=10`.
//!
//! Values are written as in the `k3d` text format: a number, a list as `a,b,c`, `true` or
//! `false`, or a text. Where the schema of the method says what a parameter takes, that
//! decides, so that a text with a comma in it or a name made of digits needs no quoting.

use krly3d_kernel::schema::{Kind, Params, Value};
use krly3d_kernel::Method;

/// The `key=value` words of a command line as the arguments of `method`. The method need
/// not be among `methods`: its values are then read by their looks alone and the session
/// says that it does not know the method.
pub fn params(methods: &[Method], method: &str, words: &[String]) -> Result<Params, String> {
    let schema = methods.iter().find(|listed| listed.name == method).map(|listed| &listed.schema);
    let mut params = Params::new();
    for word in words {
        let (name, text) = match word.split_once('=') {
            Some((name, text)) if !name.is_empty() => (name, text),
            _ => return Err(format!("`{word}` is not an argument; arguments are written key=value")),
        };
        if params.get(name).is_some() {
            return Err(format!("`{name}` is given twice"));
        }
        let kind = schema.and_then(|schema| schema.params.iter().find(|param| param.name == name));
        params.set(name, value(kind.map(|param| param.kind), text));
    }
    Ok(params)
}

fn value(kind: Option<Kind>, text: &str) -> Value {
    if let Some(quoted) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Value::Text(quoted.to_string());
    }
    match kind {
        Some(Kind::Text) => Value::Text(text.to_string()),
        Some(Kind::Texts) => Value::List(items(text).map(Value::from).collect()),
        _ if text.contains(',') => Value::List(items(text).map(scalar).collect()),
        _ => scalar(text),
    }
}

/// The items of a list; a trailing comma makes a list of one.
fn items(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').filter(|item| !item.is_empty())
}

fn scalar(text: &str) -> Value {
    // Rust reads `inf` and `nan` as numbers; here they are words like any other.
    let numeric = text.starts_with(|first: char| first.is_ascii_digit() || matches!(first, '-' | '+' | '.'));
    match (text, text.parse::<f64>()) {
        ("true", _) => Value::Bool(true),
        ("false", _) => Value::Bool(false),
        (_, Ok(number)) if numeric && number.is_finite() => Value::Number(number),
        _ => Value::Text(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::params;
    use krly3d_kernel::schema::{Kind, Param, Schema, Value};
    use krly3d_kernel::{Method, MethodKind};

    fn methods() -> Vec<Method> {
        let schema = Schema {
            about: "a drawn thing",
            inputs: (0, 0),
            params: vec![
                Param::required("path", Kind::Text, "the outline"),
                Param::optional("id", Kind::Text, "", "name of the node"),
                Param::optional("inputs", Kind::Texts, Vec::new(), "nodes read"),
                Param::optional("r", Kind::Number, 1.0, "radius"),
                Param::optional("view", Kind::Any, "iso", "a word or a direction"),
            ],
        };
        vec![Method { name: "draw".to_string(), kind: MethodKind::Tool, schema }]
    }

    fn read(words: &[&str]) -> Result<Vec<(String, Value)>, String> {
        let words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        let params = params(&methods(), "draw", &words)?;
        Ok(params.iter().map(|(name, value)| (name.to_string(), value.clone())).collect())
    }

    #[test]
    fn values_are_read_as_number_list_flag_or_text() {
        let read = read(&["r=2.5", "at=1,-2,0", "dry=true", "plane=xz"]).unwrap();

        assert_eq!(
            read,
            [
                ("at".to_string(), Value::from([1.0, -2.0, 0.0])),
                ("dry".to_string(), Value::from(true)),
                ("plane".to_string(), Value::from("xz")),
                ("r".to_string(), Value::from(2.5)),
            ]
        );
    }

    #[test]
    fn a_text_parameter_keeps_its_commas_and_digits() {
        let read = read(&["path=M 0,0 L 4,0 L 4,2 Z", "id=42"]).unwrap();

        assert_eq!(
            read,
            [("id".to_string(), Value::from("42")), ("path".to_string(), Value::from("M 0,0 L 4,0 L 4,2 Z")),]
        );
    }

    #[test]
    fn a_texts_parameter_is_split_at_commas_into_texts() {
        let read = read(&["inputs=plate,7"]).unwrap();

        assert_eq!(read, [("inputs".to_string(), Value::texts(["plate", "7"]))]);
    }

    #[test]
    fn a_quoted_value_is_a_text_whatever_it_looks_like() {
        let read = read(&["view=\"1,2,3\"", "r=\"5\""]).unwrap();

        assert_eq!(read, [("r".to_string(), Value::from("5")), ("view".to_string(), Value::from("1,2,3"))]);
    }

    #[test]
    fn a_parameter_of_any_kind_is_read_by_its_looks() {
        let word = read(&["view=front"]).unwrap();
        let triple = read(&["view=1,1,-1"]).unwrap();

        assert_eq!(word, [("view".to_string(), Value::from("front"))]);
        assert_eq!(triple, [("view".to_string(), Value::from([1.0, 1.0, -1.0]))]);
    }

    #[test]
    fn words_that_only_look_like_numbers_stay_text() {
        let read = read(&["a=inf", "b=nan", "c=-inf", "d=1e999"]).unwrap();

        let texts: Vec<Value> = ["inf", "nan", "-inf", "1e999"].into_iter().map(Value::from).collect();
        assert_eq!(read.into_iter().map(|(_, value)| value).collect::<Vec<_>>(), texts);
    }

    #[test]
    fn a_word_without_a_key_is_refused() {
        let bare = read(&["plate"]).unwrap_err();
        let keyless = read(&["=4"]).unwrap_err();

        assert_eq!(bare, "`plate` is not an argument; arguments are written key=value");
        assert_eq!(keyless, "`=4` is not an argument; arguments are written key=value");
    }

    #[test]
    fn an_argument_given_twice_is_refused() {
        let error = read(&["r=1", "r=2"]).unwrap_err();

        assert_eq!(error, "`r` is given twice");
    }

    #[test]
    fn the_arguments_of_an_unknown_method_are_read_by_their_looks() {
        let words = ["w=60".to_string(), "name=a,b".to_string()];

        let params = params(&methods(), "unknown", &words).unwrap();

        assert_eq!(params.number("w").unwrap(), 60.0);
        assert_eq!(params.texts("name").unwrap(), ["a", "b"]);
    }
}
