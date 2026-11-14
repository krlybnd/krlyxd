//! Looking up rules that relate two kinds.

use crate::context::Context;
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};

/// Asks the rules of family `F` keyed `first|second`, from the most specific pair of
/// classes to the most general. The classes of `first` vary slowest.
///
/// This is what replaces a table of case branches: knowledge about a pair lives in the
/// rule registered under its key, and a general rule under a general class catches the rest.
pub fn ask<F: Family<Context = Context>>(
    cx: &Context,
    first: &[&str],
    second: &[&str],
    request: &F::Request,
) -> Attempt<F::Response> {
    let mut reasons = Vec::new();
    for a in first {
        for b in second {
            let id = Id::new(&format!("{a}|{b}"));
            if !cx.registry::<F>().is_some_and(|registry| registry.contains(&id)) {
                continue;
            }
            match cx.call::<F>(&id, request) {
                Attempt::Declined(reason) => reasons.push(reason),
                settled => return settled,
            }
        }
    }
    if reasons.is_empty() {
        Attempt::declined(format!("no {} rule for `{}|{}`", F::NAME, kind(first), kind(second)))
    } else {
        Attempt::declined(reasons.join("; "))
    }
}

/// Asks the rules of family `F` keyed by a single class, most specific class first.
pub fn ask_class<F: Family<Context = Context>>(
    cx: &Context,
    classes: &[&str],
    request: &F::Request,
) -> Attempt<F::Response> {
    let mut reasons = Vec::new();
    for class in classes {
        let id = Id::new(class);
        if !cx.registry::<F>().is_some_and(|registry| registry.contains(&id)) {
            continue;
        }
        match cx.call::<F>(&id, request) {
            Attempt::Declined(reason) => reasons.push(reason),
            settled => return settled,
        }
    }
    if reasons.is_empty() {
        Attempt::declined(format!("no {} rule for `{}`", F::NAME, kind(classes)))
    } else {
        Attempt::declined(reasons.join("; "))
    }
}

/// The kind is the most specific class.
fn kind<'a>(classes: &[&'a str]) -> &'a str {
    classes.first().copied().unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::ask;
    use crate::builder::KernelBuilder;
    use crate::context::Context;
    use crate::plugin::{Family, Plugin};
    use krly3d_core::{Attempt, Id};

    struct Pairing;

    impl Family for Pairing {
        const NAME: &'static str = "pairing";
        type Request = ();
        type Response = &'static str;
        type Context = crate::context::Context;
    }

    struct Rule {
        id: &'static str,
        answer: Attempt<&'static str>,
    }

    impl Plugin<Pairing> for Rule {
        fn id(&self) -> Id {
            Id::new(self.id)
        }

        fn call(&self, _: &Context, _: &()) -> Attempt<&'static str> {
            self.answer.clone()
        }
    }

    fn context(rules: Vec<Rule>) -> Context {
        let mut builder = KernelBuilder::new();
        for rule in rules {
            builder.plugin::<Pairing>(rule);
        }
        builder.context()
    }

    #[test]
    fn the_most_specific_pair_is_asked_first() {
        let cx = context(vec![
            Rule { id: "curve|surface", answer: Attempt::Done("general") },
            Rule { id: "line|sphere", answer: Attempt::Done("specific") },
        ]);

        let answer = ask::<Pairing>(&cx, &["line", "curve"], &["sphere", "quadric", "surface"], &());

        assert_eq!(answer, Attempt::Done("specific"));
    }

    #[test]
    fn a_general_rule_catches_what_the_specific_one_declines() {
        let cx = context(vec![
            Rule { id: "line|sphere", answer: Attempt::declined("tangent") },
            Rule { id: "line|surface", answer: Attempt::Done("general") },
        ]);

        let answer = ask::<Pairing>(&cx, &["line", "curve"], &["sphere", "quadric", "surface"], &());

        assert_eq!(answer, Attempt::Done("general"));
    }

    #[test]
    fn a_pair_without_any_rule_is_declined_by_its_kinds() {
        let cx = context(vec![]);

        let answer = ask::<Pairing>(&cx, &["helix", "curve"], &["torus", "surface"], &());

        assert_eq!(answer, Attempt::declined("no pairing rule for `helix|torus`"));
    }
}
