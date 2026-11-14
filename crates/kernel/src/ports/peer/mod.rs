//! Another server, asked from inside the kernel.
//!
//! A tool, a query or a claim may need what another program knows: the outline of a
//! photograph, a camera. The kernel does not know that the answer comes from another
//! process; it asks this family under the peer's name (`vis`), and whatever serves the
//! session — the command line, a test — registers the plugin that carries the question
//! over. Every answer is a [`Reply`], as if the method had been called by a client.

use crate::api::Reply;
use crate::context::Context;
use crate::plugin::Family;
use crate::schema::Params;
use krly3d_core::{Attempt, Error, Id};

pub struct Peer;

/// A method of the peer and its arguments, as a client would call it.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerCall {
    pub method: String,
    pub params: Params,
}

impl Family for Peer {
    const NAME: &'static str = "peer";
    type Request = PeerCall;
    type Response = Reply;
    type Context = Context;
}

/// Asks the peer named `peer` to answer `method`. A reply that is not `ok` fails with
/// the peer's own words, so that a caller can pass the reason on.
pub fn ask(cx: &Context, peer: &str, method: &str, params: Params) -> Attempt<Reply> {
    let call = PeerCall { method: method.to_string(), params };
    match cx.call::<Peer>(&Id::new(peer), &call) {
        Attempt::Done(reply) if reply.ok => Attempt::Done(reply),
        Attempt::Done(reply) => {
            Attempt::Failed(Error::input(format!("{peer}.{method}: {}", reply.text.trim_end())))
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{ask, Peer, PeerCall};
    use crate::api::Reply;
    use crate::context::Context;
    use crate::plugin::Plugin;
    use crate::schema::{Params, Value};
    use crate::KernelBuilder;
    use krly3d_core::{Attempt, Exactness, Id};

    /// A peer that answers `echo` with the text it was given and refuses anything else.
    struct Echo;

    impl Plugin<Peer> for Echo {
        fn id(&self) -> Id {
            Id::new("vis")
        }

        fn call(&self, _: &Context, request: &PeerCall) -> Attempt<Reply> {
            let ok = request.method == "echo";
            let text = match request.params.get("text") {
                Some(Value::Text(text)) if ok => text.clone(),
                _ => format!("there is no method `{}`", request.method),
            };
            Attempt::Done(Reply { ok, text, data: Value::map([]), exact: Exactness::Exact })
        }
    }

    fn context() -> Context {
        let mut builder = KernelBuilder::new();
        builder.plugin::<Peer>(Echo);
        builder.context()
    }

    #[test]
    fn a_peer_is_asked_by_its_name_and_answers_as_to_a_client() {
        let reply =
            ask(&context(), "vis", "echo", Params::new().with("text", "hello")).into_result().unwrap();

        assert_eq!(reply.text, "hello");
    }

    #[test]
    fn a_refusal_of_the_peer_fails_with_its_reason() {
        let refused = ask(&context(), "vis", "nothing", Params::new()).into_result().unwrap_err();

        assert!(refused.message.contains("vis.nothing: there is no method `nothing`"), "{}", refused.message);
    }

    #[test]
    fn a_peer_that_was_never_joined_is_declined() {
        let declined = ask(&context(), "photo", "echo", Params::new());

        assert!(matches!(declined, Attempt::Declined(_)));
    }
}
