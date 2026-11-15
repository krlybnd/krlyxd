//! What a transport serves: anything that lists its methods and answers calls to them.
//!
//! The krly3d session is one endpoint. Another server, krlyvis, is another; a server
//! reached over JSON-RPC and joined to this one is a third (see [`crate::peer`]). The
//! dialects, the shell form and the command line work with all of them alike.

use krly3d_core::CallControl;
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Method, Reply, Session};
use std::sync::Arc;

pub trait Endpoint {
    /// The name a client knows the server by.
    fn name(&self) -> &str;

    /// What an agent should know before calling anything, in a few sentences.
    fn instructions(&self) -> String;

    /// Every method with its arguments.
    fn methods(&self) -> Vec<Method>;

    /// Calls `method`; whatever goes wrong comes back as a reply with `ok` false.
    fn call(&mut self, method: &str, args: &Params) -> Reply;

    /// Opt-in cooperative cancellation. Existing endpoints retain their call behavior;
    /// implementations that delegate local calls must forward the control as well.
    fn call_with_control(&mut self, method: &str, args: &Params, _control: Arc<CallControl>) -> Reply {
        self.call(method, args)
    }
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

    fn call_with_control(&mut self, method: &str, args: &Params, control: Arc<CallControl>) -> Reply {
        Session::call_with_control(self, method, args, control)
    }
}

/// A connection's control, presented to unchanged dialects through `Endpoint::call`.
pub(crate) struct Controlled<'a> {
    pub(crate) endpoint: &'a mut dyn Endpoint,
    pub(crate) control: Arc<CallControl>,
}

impl Endpoint for Controlled<'_> {
    fn name(&self) -> &str {
        self.endpoint.name()
    }

    fn instructions(&self) -> String {
        self.endpoint.instructions()
    }

    fn methods(&self) -> Vec<Method> {
        self.endpoint.methods()
    }

    fn call(&mut self, method: &str, args: &Params) -> Reply {
        self.endpoint.call_with_control(method, args, Arc::clone(&self.control))
    }
}
