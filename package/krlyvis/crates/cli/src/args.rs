//! The command line of `krlyvis`, in the same forms as that of `krly3d`.

use krly3d_cli::dialect;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Help,
    /// Answer requests on standard input in the named dialect until it ends.
    Serve {
        dialect: String,
    },
    /// Make one call and print the text of the reply.
    Call {
        method: String,
        arguments: Vec<String>,
    },
}

pub fn parse(args: &[String]) -> Result<Action, String> {
    let mut chosen: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Action::Help),
            "call" => {
                if let Some(dialect) = chosen {
                    return Err(format!("`--{dialect}` serves requests; it does not go with `call`"));
                }
                let method = rest.next().ok_or("`call` needs a method after it")?.clone();
                return Ok(Action::Call { method, arguments: rest.cloned().collect() });
            }
            flag => {
                let name = flag.strip_prefix("--").filter(|name| dialect::named(name).is_some());
                let name = name.ok_or_else(|| format!("unknown argument `{flag}`; see --help"))?;
                if let Some(first) = chosen.replace(name.to_string()).filter(|first| first != name) {
                    return Err(format!("`--{first}` and `--{name}` are two dialects; choose one"));
                }
            }
        }
    }
    let default = dialect::all().first().map(|dialect| dialect.name().to_string());
    Ok(Action::Serve { dialect: chosen.or(default).ok_or("no dialect is built in")? })
}

pub fn usage() -> String {
    let mut text = String::from(
        "krlyvis — straight edges, vanishing points, camera and lengths from one picture\n\
         \n\
         usage:\n\
         \x20 krlyvis [--<dialect>]             answer requests on standard input, one per line\n\
         \x20 krlyvis call <method> key=value   make one call and print the text of the reply\n\
         \x20 krlyvis --help\n\
         \n\
         dialects:\n",
    );
    for dialect in dialect::all() {
        text.push_str(&format!("  --{:<13} {}\n", dialect.name(), dialect.about()));
    }
    text.push_str(
        "\n\
         example:\n\
         \x20 krlyvis call analyse path=bridge.pgm\n\
         \x20 krlyvis call analyse path=bridge.pgm plane=xz known=s12=4.5\n\
         \n\
         `krlyvis call methods` lists every method with its arguments. krly3d answers these\n\
         methods too when started as `krly3d --peer vis=krlyvis`, under the names `vis.…`.\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::{parse, usage, Action};

    fn parsed(line: &str) -> Result<Action, String> {
        parse(&line.split_whitespace().map(str::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_serve_the_plain_dialect() {
        assert_eq!(parsed(""), Ok(Action::Serve { dialect: "plain".to_string() }));
        assert_eq!(parsed("--mcp"), Ok(Action::Serve { dialect: "mcp".to_string() }));
    }

    #[test]
    fn call_takes_the_method_and_the_rest_as_arguments() {
        assert_eq!(
            parsed("call analyse path=a.pgm known=s1=2"),
            Ok(Action::Call {
                method: "analyse".to_string(),
                arguments: vec!["path=a.pgm".to_string(), "known=s1=2".to_string()]
            })
        );
    }

    #[test]
    fn wrong_command_lines_are_refused() {
        for line in ["--frobnicate", "--mcp call analyse", "--plain --mcp", "call"] {
            assert!(parsed(line).is_err(), "{line}");
        }
        assert_eq!(parsed("--help"), Ok(Action::Help));
        assert!(usage().contains("--mcp"));
    }
}
