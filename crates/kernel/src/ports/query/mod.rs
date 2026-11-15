//! Port of the `query` family: a question about the model, answered as a short text plus
//! the same content as data. Queries are keyed by name and never change anything.

pub mod contract;

use crate::context::Context;
use crate::model::{Built, Document, Model, NodeRef};
use crate::plugin::Family;
use crate::schema::{Params, Value};
use krly3d_core::{Attempt, Error, Exactness, Id};
use std::sync::Arc;

pub struct Query;

impl Family for Query {
    const NAME: &'static str = "query";
    type Request = QueryRequest;
    type Response = Answer;
    type Context = crate::context::Context;
}

pub struct QueryRequest {
    /// Already admitted by the query's schema: complete and of the right kinds.
    pub params: Params,
    pub document: Arc<Document>,
    pub model: Arc<Model>,
}

impl QueryRequest {
    /// The built node named by the text parameter `name` of the request, as `part` or `part@2`.
    pub fn node(&self, name: &str) -> Result<&Built, Error> {
        let reference = NodeRef::parse(self.params.text(name)?)?;
        self.model
            .built(&reference)
            .ok_or_else(|| Error::not_found(format!("there is no node `{reference}`")))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// The answer for a reader of text: compact, with names and exact numbers.
    pub text: String,
    /// The same answer as data.
    pub data: Value,
    /// How far the numbers in the answer can be trusted.
    pub exact: Exactness,
}

/// Asks the query registered as `query`, after admitting the parameters through its schema.
pub fn ask(
    cx: &Context,
    query: &Id,
    params: &Params,
    document: &Arc<Document>,
    model: &Arc<Model>,
) -> Attempt<Answer> {
    if let Err(error) = cx.check_control() {
        return Attempt::Failed(error);
    }
    let Some(schema) = cx.registry::<Query>().and_then(|registry| registry.schema(query)) else {
        return Attempt::declined(format!("no query is registered as `{query}`"));
    };
    match schema.admit(query, params, 0) {
        Ok(params) => {
            let request = QueryRequest { params, document: Arc::clone(document), model: Arc::clone(model) };
            let answer = cx.call::<Query>(query, &request);
            // Even a query without internal checkpoints cannot publish a successful
            // answer after it observes a stop at this final read-only boundary.
            match cx.publish_control(|| answer) {
                Ok(answer) => answer,
                Err(error) => Attempt::Failed(error),
            }
        }
        Err(error) => Attempt::Failed(error),
    }
}
