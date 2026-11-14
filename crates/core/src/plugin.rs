//! The one mould every plugin of every krly3d kernel is cast in.
//!
//! A family names a question (`Request`) and its answer (`Response`). A plugin answers
//! that question under a text id. Several plugins may share an id: they form a chain,
//! asked in registration order until one of them settles the request.

use crate::schema::Schema;
use crate::{Attempt, Id};
use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Marker type of a plugin family.
pub trait Family: 'static {
    /// Name used in messages, e.g. `section`.
    const NAME: &'static str;
    type Request;
    type Response;
    /// What every plugin of the family is handed besides the request: the kernel's
    /// context for the geometry families, another kernel's for its own.
    type Context;
}

pub trait Plugin<F: Family>: Send + Sync + 'static {
    /// Stable text id, e.g. `extrude` or `plane|sphere`.
    fn id(&self) -> Id;

    /// What the plugin takes, for the plugins that are called from outside the kernel.
    fn schema(&self) -> Option<Schema> {
        None
    }

    fn call(&self, cx: &F::Context, request: &F::Request) -> Attempt<F::Response>;
}

/// The plugins of one family, keyed by id.
pub struct Registry<F: Family> {
    chains: BTreeMap<Id, Vec<Arc<dyn Plugin<F>>>>,
}

impl<F: Family> Default for Registry<F> {
    fn default() -> Self {
        Self { chains: BTreeMap::new() }
    }
}

impl<F: Family> Registry<F> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a plugin behind the ones already registered under the same id.
    pub fn add(&mut self, plugin: impl Plugin<F>) -> &mut Self {
        self.chains.entry(plugin.id()).or_default().push(Arc::new(plugin));
        self
    }

    /// Registered ids in text order, independent of registration order.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.chains.keys()
    }

    pub fn contains(&self, id: &Id) -> bool {
        self.chains.contains_key(id)
    }

    /// The schema of the plugins registered under `id`. Plugins that share an id answer
    /// the same call, so the first one speaks for the chain.
    pub fn schema(&self, id: &Id) -> Option<Schema> {
        self.chains.get(id)?.first()?.schema()
    }

    /// Asks the chain registered under `id`.
    ///
    /// The first plugin that does not decline settles the request, whether it is done
    /// or failed: a failure is reported as it is instead of being hidden behind a
    /// fallback. When every plugin declines, the reasons are returned together.
    pub fn call(&self, cx: &F::Context, id: &Id, request: &F::Request) -> Attempt<F::Response> {
        let Some(chain) = self.chains.get(id) else {
            return Attempt::declined(format!("no {} plugin is registered as `{id}`", F::NAME));
        };
        let mut reasons = Vec::with_capacity(chain.len());
        for plugin in chain {
            match plugin.call(cx, request) {
                Attempt::Declined(reason) => reasons.push(reason),
                settled => return settled,
            }
        }
        Attempt::declined(format!("{} `{id}` declined: {}", F::NAME, reasons.join("; ")))
    }
}

/// One registry per family, found by the family's marker type, so that adding a family
/// changes nothing here.
#[derive(Default)]
pub struct Registries {
    by_family: BTreeMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Registries {
    pub fn get<F: Family>(&self) -> Option<&Registry<F>> {
        self.by_family.get(&TypeId::of::<F>())?.downcast_ref()
    }

    pub fn add<F: Family>(&mut self, plugin: impl Plugin<F>) {
        let slot = self.by_family.entry(TypeId::of::<F>()).or_insert_with(|| Box::new(Registry::<F>::new()));
        // The slot stored under the type id of `F` is always a `Registry<F>`, because this
        // function is the only place that fills it.
        if let Some(registry) = slot.downcast_mut::<Registry<F>>() {
            registry.add(plugin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Family, Plugin, Registries, Registry};
    use crate::{Attempt, Error, Id};

    struct Echo;

    /// The tests need no context; a kernel hands its own.
    struct Context;

    impl Family for Echo {
        const NAME: &'static str = "echo";
        type Request = i32;
        type Response = String;
        type Context = Context;
    }

    /// Answers every request the same way, under the given id.
    struct Fixed {
        id: &'static str,
        answer: Attempt<String>,
    }

    impl Plugin<Echo> for Fixed {
        fn id(&self) -> Id {
            Id::new(self.id)
        }

        fn call(&self, _: &Context, _: &i32) -> Attempt<String> {
            self.answer.clone()
        }
    }

    fn done(id: &'static str, text: &str) -> Fixed {
        Fixed { id, answer: Attempt::Done(text.to_string()) }
    }

    fn declines(id: &'static str, reason: &str) -> Fixed {
        Fixed { id, answer: Attempt::declined(reason) }
    }

    fn context() -> Context {
        Context
    }

    #[test]
    fn the_first_plugin_that_is_done_answers() {
        let mut registry = Registry::<Echo>::new();
        registry.add(done("say", "first")).add(done("say", "second"));

        let answer = registry.call(&context(), &Id::new("say"), &0);

        assert_eq!(answer, Attempt::Done("first".to_string()));
    }

    #[test]
    fn a_declined_request_passes_to_the_next_plugin() {
        let mut registry = Registry::<Echo>::new();
        registry.add(declines("say", "not mine")).add(done("say", "general"));

        let answer = registry.call(&context(), &Id::new("say"), &0);

        assert_eq!(answer, Attempt::Done("general".to_string()));
    }

    #[test]
    fn a_failure_stops_the_chain() {
        let mut registry = Registry::<Echo>::new();
        registry
            .add(Fixed { id: "say", answer: Attempt::Failed(Error::geometry("zero radius")) })
            .add(done("say", "fallback"));

        let answer = registry.call(&context(), &Id::new("say"), &0);

        assert_eq!(answer, Attempt::Failed(Error::geometry("zero radius")));
    }

    #[test]
    fn when_every_plugin_declines_all_reasons_are_reported() {
        let mut registry = Registry::<Echo>::new();
        registry.add(declines("say", "too small")).add(declines("say", "too curved"));

        let answer = registry.call(&context(), &Id::new("say"), &0);

        assert_eq!(answer, Attempt::declined("echo `say` declined: too small; too curved"));
    }

    #[test]
    fn an_unknown_id_is_declined_by_name() {
        let registry = Registry::<Echo>::new();

        let answer = registry.call(&context(), &Id::new("sing"), &0);

        assert_eq!(answer, Attempt::declined("no echo plugin is registered as `sing`"));
    }

    #[test]
    fn ids_are_listed_in_text_order_whatever_the_registration_order() {
        let mut registry = Registry::<Echo>::new();
        registry.add(done("torus", "t")).add(done("cone", "c")).add(done("plane", "p"));

        let ids: Vec<&str> = registry.ids().map(Id::as_str).collect();

        assert_eq!(ids, ["cone", "plane", "torus"]);
        assert!(registry.contains(&Id::new("cone")));
    }

    #[test]
    fn registries_find_the_registry_of_a_family_by_its_marker_type() {
        let mut registries = Registries::default();
        registries.add::<Echo>(done("say", "hello"));

        let answer = registries.get::<Echo>().map(|registry| registry.call(&context(), &Id::new("say"), &0));

        assert_eq!(answer, Some(Attempt::Done("hello".to_string())));
    }

    #[test]
    fn a_family_without_any_plugin_has_no_registry() {
        assert!(Registries::default().get::<Echo>().is_none());
    }
}
