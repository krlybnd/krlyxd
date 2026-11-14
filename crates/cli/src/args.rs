//! The command line of `krly3d`.

use crate::dialect;

/// The characteristic size of the model, in its own units, when `--scale` is not given.
/// The tolerances of the kernel follow from it.
pub const DEFAULT_SCALE: f64 = 100.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    /// A model file to load before anything else.
    pub open: Option<String>,
    pub scale: f64,
    /// Other servers to start and join, as (prefix, program).
    pub peers: Vec<(String, String)>,
    /// A Unix domain socket to serve on instead of standard input.
    pub listen: Option<String>,
    pub action: Action,
}

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
    /// Send standard input to the server at a socket and print its answers.
    Send {
        socket: String,
    },
}

/// Reads the arguments that follow the program name.
pub fn parse(args: &[String]) -> Result<Invocation, String> {
    let mut invocation = Invocation {
        open: None,
        scale: DEFAULT_SCALE,
        peers: Vec::new(),
        listen: None,
        action: Action::Help,
    };
    let mut chosen: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(invocation),
            "--open" => invocation.open = Some(stated(&mut rest, "--open", "a model file")?.clone()),
            "--scale" => invocation.scale = scale(stated(&mut rest, "--scale", "a size")?)?,
            "--peer" => invocation.peers.push(peer(stated(&mut rest, "--peer", "name=program")?)?),
            "--listen" => invocation.listen = Some(stated(&mut rest, "--listen", "a socket path")?.clone()),
            "send" => {
                let socket = stated(&mut rest, "send", "a socket path")?.clone();
                if let Some(extra) = rest.next() {
                    return Err(format!("`send` takes only the socket path, not `{extra}`"));
                }
                invocation.action = Action::Send { socket };
                return Ok(invocation);
            }
            "call" => {
                if let Some(dialect) = chosen {
                    return Err(format!("`--{dialect}` serves requests; it does not go with `call`"));
                }
                let method = stated(&mut rest, "call", "a method")?.clone();
                invocation.action = Action::Call { method, arguments: rest.cloned().collect() };
                return Ok(invocation);
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
    let dialect = chosen.or(default).ok_or("no dialect is built in")?;
    invocation.action = Action::Serve { dialect };
    Ok(invocation)
}

fn stated<'a>(
    rest: &mut impl Iterator<Item = &'a String>,
    after: &str,
    what: &str,
) -> Result<&'a String, String> {
    rest.next().ok_or_else(|| format!("`{after}` needs {what} after it"))
}

/// `vis=krlyvis`: the prefix of the peer's methods and the program to start.
fn peer(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((name, program)) if !name.is_empty() && !program.is_empty() => {
            Ok((name.to_string(), program.to_string()))
        }
        _ => Err(format!("`--peer` takes name=program, got `{text}`")),
    }
}

fn scale(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .ok_or_else(|| format!("`--scale` needs a positive size, got `{text}`"))
}

/// The help text. The dialects are listed as they are registered.
pub fn usage() -> String {
    let mut text = String::from(
        "krly3d — an exact B-rep geometry kernel behind one interface\n\
         \n\
         usage:\n\
         \x20 krly3d [options] [--<dialect>]            answer requests on standard input, one per line\n\
         \x20 krly3d [options] call <method> key=value  make one call and print the text of the reply\n\
         \x20 krly3d send <socket>                     send standard input to a listening server, print its answers\n\
         \x20 krly3d --help\n\
         \n\
         options:\n\
         \x20 --open <file>   load a model file first; its extension names the format\n\
         \x20 --scale <size>  characteristic size of the model, from which tolerances follow\n\
         \x20 --peer <name>=<program>\n\
         \x20                 start another server, such as `vis=krlyvis`, and answer its methods\n\
         \x20                 as `<name>.<method>` over JSON-RPC; may be given more than once\n\
         \x20 --listen <socket>\n\
         \x20                 serve on a Unix domain socket instead of standard input; the model and\n\
         \x20                 the peers last from one client to the next\n\
         \n\
         dialects:\n",
    );
    for dialect in dialect::all() {
        text.push_str(&format!("  --{:<13} {}\n", dialect.name(), dialect.about()));
    }
    text.push_str(
        "\n\
         In the shell form a value is a number, a list a,b,c, true or false, or a text.\n\
         `krly3d call methods` lists every method with its arguments.\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::{parse, usage, Action, Invocation, DEFAULT_SCALE};

    fn parsed(args: &[&str]) -> Result<Invocation, String> {
        parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_serve_the_plain_dialect() {
        let invocation = parsed(&[]).unwrap();

        assert_eq!(
            invocation,
            Invocation {
                open: None,
                scale: DEFAULT_SCALE,
                peers: Vec::new(),
                listen: None,
                action: Action::Serve { dialect: "plain".to_string() }
            }
        );
    }

    #[test]
    fn a_dialect_flag_serves_that_dialect() {
        let invocation = parsed(&["--mcp"]).unwrap();

        assert_eq!(invocation.action, Action::Serve { dialect: "mcp".to_string() });
    }

    #[test]
    fn a_model_file_and_a_scale_go_with_either_way_of_running() {
        let serving = parsed(&["--open", "model.k3d", "--scale", "2500", "--mcp"]).unwrap();
        let calling = parsed(&["--open", "model.k3d", "call", "save"]).unwrap();

        assert_eq!((serving.open.as_deref(), serving.scale), (Some("model.k3d"), 2500.0));
        assert_eq!(calling.open.as_deref(), Some("model.k3d"));
        assert_eq!(calling.action, Action::Call { method: "save".to_string(), arguments: Vec::new() });
    }

    #[test]
    fn everything_after_the_method_of_a_call_is_its_arguments() {
        let invocation = parsed(&["call", "box", "w=60", "--mcp", "id=--help"]).unwrap();

        assert_eq!(
            invocation.action,
            Action::Call {
                method: "box".to_string(),
                arguments: vec!["w=60".to_string(), "--mcp".to_string(), "id=--help".to_string()]
            }
        );
    }

    #[test]
    fn help_is_asked_for_with_a_flag() {
        assert_eq!(parsed(&["--help"]).unwrap().action, Action::Help);
        assert_eq!(parsed(&["--mcp", "-h"]).unwrap().action, Action::Help);
    }

    #[test]
    fn an_argument_nobody_knows_is_refused_by_name() {
        let error = parsed(&["--verbose"]).unwrap_err();

        assert_eq!(error, "unknown argument `--verbose`; see --help");
    }

    #[test]
    fn an_option_without_its_value_is_refused() {
        assert_eq!(parsed(&["--open"]).unwrap_err(), "`--open` needs a model file after it");
        assert_eq!(parsed(&["call"]).unwrap_err(), "`call` needs a method after it");
    }

    #[test]
    fn a_scale_that_is_not_a_positive_size_is_refused() {
        for text in ["0", "-5", "big", "inf", "nan"] {
            let error = parsed(&["--scale", text]).unwrap_err();

            assert_eq!(error, format!("`--scale` needs a positive size, got `{text}`"));
        }
    }

    #[test]
    fn two_dialects_at_once_are_refused() {
        let error = parsed(&["--plain", "--mcp"]).unwrap_err();

        assert_eq!(error, "`--plain` and `--mcp` are two dialects; choose one");
    }

    #[test]
    fn a_dialect_does_not_go_with_a_single_call() {
        let error = parsed(&["--mcp", "call", "save"]).unwrap_err();

        assert_eq!(error, "`--mcp` serves requests; it does not go with `call`");
    }

    #[test]
    fn the_help_text_lists_every_dialect() {
        let text = usage();

        assert!(text.contains("--plain") && text.contains("--mcp"), "{text}");
    }
}
