//! What a transport serves: anything that lists its methods and answers calls to them.
//!
//! The krly3d session is one endpoint. Another server, krlyvis, is another; a server
//! reached over JSON-RPC and joined to this one is a third (see [`crate::peer`]). The
//! dialects, the shell form and the command line work with all of them alike.

use krly3d_kernel::schema::Params;
use krly3d_kernel::{Method, Reply, Session};

pub trait Endpoint {
    /// The name a client knows the server by.
    fn name(&self) -> &str;

    /// What an agent should know before calling anything, in a few sentences.
    fn instructions(&self) -> String;

    /// Every method with its arguments.
    fn methods(&self) -> Vec<Method>;

    /// Calls `method`; whatever goes wrong comes back as a reply with `ok` false.
    fn call(&mut self, method: &str, args: &Params) -> Reply;
}

const INSTRUCTIONS: &str = "krly3d is an exact B-rep geometry kernel. Tools add statements to one model \
     that lasts as long as the connection; queries answer questions about it and change nothing. \
     Every answer is a short text: in it `=` marks a closed-form number, `~` a numerical one and `?` \
     a sampled one. Lengths have no unit, angles are in degrees, z is up.";

impl Endpoint for Session {
    fn name(&self) -> &str {
        "krly3d"
    }

    fn instructions(&self) -> String {
        INSTRUCTIONS.to_string()
    }

    fn methods(&self) -> Vec<Method> {
        Session::methods(self)
    }

    fn call(&mut self, method: &str, args: &Params) -> Reply {
        Session::call(self, method, args)
    }
}
