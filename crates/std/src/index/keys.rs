//! The index of keys and relations: for every face what the key families say about it
//! (K), and which faces are next to it or related to it by a key family (R).
//!
//! It is built in one pass over the faces and one over the edges, the first time a body
//! is searched, and kept for as long as the body lives.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::index::{Index, IndexRequest, Indexing, Search, ALPHA, HOPS, KEYS};
use krly3d_kernel::ports::keys::{keys, KeysRequest};
use krly3d_kernel::topo::{anchor, body_bounds, edge_faces, Body};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct KeyIndexer;

impl Plugin<Indexing> for KeyIndexer {
    fn id(&self) -> Id {
        Id::new(KEYS)
    }

    fn call(&self, cx: &Context, request: &IndexRequest) -> Attempt<Arc<dyn Index>> {
        cx.derived(&request.body, || Attempt::from(KeyIndex::of(cx, &request.body)))
            .map(|index| index as Arc<dyn Index>)
    }
}

#[derive(Debug)]
struct KeyIndex {
    elements: Vec<String>,
    /// The rows of K: the keys of each element.
    keys: Vec<Vec<String>>,
    /// The rows of R: the elements related to each element, in ascending order.
    related: Vec<Vec<usize>>,
    /// The columns of K: for every key the elements that have it.
    holders: BTreeMap<String, Vec<usize>>,
}

impl KeyIndex {
    fn of(cx: &Context, body: &Arc<Body>) -> Result<Self, Error> {
        let bounds = body_bounds(cx, body).into_result()?;
        let mut index = Self {
            elements: body.faces.iter().map(|face| face.name.clone()).collect(),
            keys: Vec::with_capacity(body.faces.len()),
            related: vec![Vec::new(); body.faces.len()],
            holders: BTreeMap::new(),
        };
        for face in 0..body.faces.len() {
            let anchor = anchor(cx, body, face).into_result()?;
            let mut said =
                keys(cx, &KeysRequest { body: Arc::clone(body), face, anchor, bounds }).into_result()?;
            said.keys.sort_unstable();
            said.keys.dedup();
            for key in &said.keys {
                index.holders.entry(key.clone()).or_default().push(face);
            }
            index.keys.push(said.keys);
            for other in said.related {
                index.relate(face, other);
            }
        }
        // Faces on the two sides of an edge are neighbours.
        for users in edge_faces(body) {
            if let [one, other] = users.as_slice() {
                index.relate(*one, *other);
            }
        }
        index.related.iter_mut().for_each(|row| row.sort_unstable());
        Ok(index)
    }

    /// Relates two different elements to each other, once.
    fn relate(&mut self, one: usize, other: usize) {
        let known = one.max(other) < self.related.len();
        if known && one != other && !self.related[one].contains(&other) {
            self.related[one].push(other);
            self.related[other].push(one);
        }
    }

    /// `K·q`: for every element the weights of the keys it has.
    fn matched(&self, terms: &[(String, f64)]) -> Vec<f64> {
        let mut matched = vec![0.0; self.elements.len()];
        for (key, weight) in terms {
            for holder in self.holders.get(key).into_iter().flatten() {
                matched[*holder] += weight;
            }
        }
        matched
    }

    /// `R·x`: every element receives what the elements related to it hold.
    fn hop(&self, held: &[f64]) -> Vec<f64> {
        self.related.iter().map(|row| row.iter().map(|other| held[*other]).sum()).collect()
    }
}

impl Index for KeyIndex {
    fn elements(&self) -> &[String] {
        &self.elements
    }

    fn keys(&self, element: usize) -> &[String] {
        self.keys.get(element).map_or(&[], Vec::as_slice)
    }

    fn related(&self, element: usize) -> &[usize] {
        self.related.get(element).map_or(&[], Vec::as_slice)
    }

    fn score(&self, search: &Search) -> Vec<f64> {
        let mut score = self.matched(&search.this);
        let (mut passed, mut weight) = (self.matched(&search.near), 1.0);
        for _ in 0..HOPS {
            passed = self.hop(&passed);
            weight *= ALPHA;
            for (total, share) in score.iter_mut().zip(&passed) {
                *total += weight * share;
            }
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{context, made};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::index::{best, contract, index, Index, Search};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use std::sync::Arc;

    /// A block with one upright corner rounded: the round face touches `right`, `back`,
    /// `top` and `bottom`, and not `front` or `left`.
    fn rounded_block(cx: &Context) -> Arc<Body> {
        let names: Vec<Value> = ["front", "right", "round", "back", "left"].map(Value::from).to_vec();
        let outline =
            Params::new().with("path", "M 0,0 L 40,0 L 40,7 A 37,7 37,10 ccw L 0,10 Z").with("names", names);
        made(cx, "extrude", Params::new().with("h", 20.0), &[made(cx, "profile", outline, &[])])
    }

    fn tube(cx: &Context) -> Arc<Body> {
        let section = Params::new().with("path", "M 2,0 L 5,0 L 5,4 L 2,4 Z").with("plane", "xz");
        made(cx, "revolve", Params::new(), &[made(cx, "profile", section, &[])])
    }

    fn names<'a>(index: &'a dyn Index, elements: &[usize]) -> Vec<&'a str> {
        let mut names: Vec<&str> =
            elements.iter().map(|element| index.elements()[*element].as_str()).collect();
        names.sort_unstable();
        names
    }

    fn element(index: &dyn Index, name: &str) -> usize {
        index.elements().iter().position(|known| known == name).unwrap()
    }

    #[test]
    fn the_index_of_every_kind_of_solid_satisfies_the_contract() {
        let cx = context();
        let solids = [
            rounded_block(&cx),
            tube(&cx),
            made(&cx, "sphere", Params::new().with("r", 5.0), &[]),
            made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]),
            made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]),
        ];
        for solid in &solids {
            let index = index(&cx, solid).into_result().unwrap();

            contract::check(index.as_ref(), solid, cx.accuracy()).unwrap();
        }
    }

    #[test]
    fn a_face_holds_the_keys_of_every_family_that_has_something_to_say_about_it() {
        let cx = context();
        let block = rounded_block(&cx);

        let index = index(&cx, &block).into_result().unwrap();

        // Where a face is has a test of its own with the cell family.
        let apart_from_where = |name: &str| -> Vec<&str> {
            let keys = index.keys(element(index.as_ref(), name));
            keys.iter().map(String::as_str).filter(|key| !key.starts_with("cell:")).collect()
        };
        let cells = |name: &str| {
            index.keys(element(index.as_ref(), name)).iter().filter(|key| key.starts_with("cell:")).count()
        };
        assert_eq!(apart_from_where("round"), ["axis:+z", "kind:cylinder", "name:round", "r:3"]);
        assert_eq!(apart_from_where("top"), ["kind:plane", "n:+z", "name:top"]);
        assert_eq!((cells("round"), cells("top")), (2, 2));
    }

    #[test]
    fn faces_that_share_an_edge_are_related() {
        let cx = context();
        let block = rounded_block(&cx);

        let index = index(&cx, &block).into_result().unwrap();

        let near_round = index.related(element(index.as_ref(), "round"));
        let near_front = index.related(element(index.as_ref(), "front"));
        assert_eq!(names(index.as_ref(), near_round), ["back", "bottom", "right", "top"]);
        assert_eq!(names(index.as_ref(), near_front), ["bottom", "left", "right", "top"]);
    }

    #[test]
    fn cylinders_about_one_axis_are_related_although_they_share_no_edge() {
        let cx = context();
        let tube = tube(&cx);

        let index = index(&cx, &tube).into_result().unwrap();

        let (bore, outside) = (element(index.as_ref(), "s4"), element(index.as_ref(), "s2"));
        assert!(index.related(bore).contains(&outside));
        assert!(index.keys(bore).contains(&"coaxial:s2".to_string()));
    }

    #[test]
    fn a_key_of_the_face_itself_counts_in_full() {
        let cx = context();
        let block = rounded_block(&cx);
        let index = index(&cx, &block).into_result().unwrap();

        let found = best(index.as_ref(), &Search::parse("cylinder").unwrap(), 5);

        assert_eq!(found, [(element(index.as_ref(), "round"), 1.0)]);
    }

    #[test]
    fn a_key_of_a_neighbour_counts_half_and_of_a_neighbours_neighbour_a_quarter_per_path() {
        let cx = context();
        let block = rounded_block(&cx);
        let index = index(&cx, &block).into_result().unwrap();

        let score = index.score(&Search::parse("near:cylinder").unwrap());

        // `right` touches the round face, and reaches it again in two steps by way of
        // `top` and of `bottom`; `front` only reaches it in two steps, three ways; the
        // round face reaches itself by way of each of its four neighbours and back.
        let of = |name: &str| score[element(index.as_ref(), name)];
        assert_eq!((of("right"), of("front"), of("round")), (0.5 + 2.0 * 0.25, 3.0 * 0.25, 4.0 * 0.25));
    }

    #[test]
    fn planes_that_touch_the_cylinder_outrank_the_planes_that_do_not() {
        let cx = context();
        let block = rounded_block(&cx);
        let index = index(&cx, &block).into_result().unwrap();

        let found = best(index.as_ref(), &Search::parse("plane near:kind:cylinder").unwrap(), 6);

        let ranked: Vec<usize> = found.iter().map(|(element, _)| *element).collect();
        assert_eq!(names(index.as_ref(), &ranked[..4]), ["back", "bottom", "right", "top"]);
        assert_eq!(names(index.as_ref(), &ranked[4..]), ["front", "left"]);
        assert_eq!(
            found.iter().map(|(_, score)| *score).collect::<Vec<_>>(),
            [2.0, 2.0, 2.0, 2.0, 1.75, 1.75]
        );
    }

    #[test]
    fn a_weight_scales_what_a_key_counts_for() {
        let cx = context();
        let block = rounded_block(&cx);
        let index = index(&cx, &block).into_result().unwrap();

        let found = best(index.as_ref(), &Search::parse("n:+z^3 plane").unwrap(), 1);

        assert_eq!(found, [(element(index.as_ref(), "top"), 4.0)]);
    }

    #[test]
    fn faces_that_match_nothing_are_left_out() {
        let cx = context();
        let block = rounded_block(&cx);
        let index = index(&cx, &block).into_result().unwrap();

        let found = best(index.as_ref(), &Search::parse("torus").unwrap(), 5);

        assert!(found.is_empty());
    }

    #[test]
    fn the_index_of_a_body_is_built_once() {
        let cx = context();
        let block = rounded_block(&cx);

        let (first, second) = (index(&cx, &block), index(&cx, &block));

        let (Attempt::Done(first), Attempt::Done(second)) = (first, second) else { panic!() };
        assert!(Arc::ptr_eq(&first, &second));
    }
}
