//! The `render` query: a look at the model as a picture.
//!
//! The file is named after what the query makes, because a module `render` inside the
//! folder `render` would only repeat the name of its parent.

use super::{base64, paint_param, png, shot_params, Shot};
use crate::ports::render::CHANNELS;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::{Context, Plugin};

pub struct Picture;

impl Plugin<Query> for Picture {
    fn id(&self) -> Id {
        Id::new("render")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a picture of the model as a PNG image: every face shaded by how it is turned, in its paint if given, dark lines where faces meet",
            inputs: (0, 0),
            params: [shot_params(), vec![paint_param()]].concat(),
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        match Shot::take(cx, request) {
            Attempt::Done(shot) => pictured(&shot).into(),
            Attempt::Declined(reason) => Attempt::Declined(reason),
            Attempt::Failed(error) => Attempt::Failed(error),
        }
    }
}

fn pictured(shot: &Shot) -> Result<Answer, Error> {
    let file = png::encode(shot.image.width(), shot.image.height(), CHANNELS, &shot.image.pixels)?;
    let mut data = shot.data();
    data.insert("png".to_string(), Value::from(base64::encode(&file)));
    Ok(Answer {
        text: format!("{}\n", shot.headline("render", shot.tally().len())),
        data: Value::Map(data),
        exact: shot.exactness(),
    })
}

#[cfg(test)]
mod tests {
    use crate::testing::session;
    use krly3d_core::{Exactness, Id};
    use krly3d_kernel::ports::query::contract;
    use krly3d_kernel::schema::{Params, Value};

    fn entry<'a>(data: &'a Value, name: &str) -> &'a Value {
        let Value::Map(entries) = data else { panic!("not a map: {data:?}") };
        entries.get(name).unwrap_or_else(|| panic!("no `{name}`"))
    }

    #[test]
    fn the_query_satisfies_the_contract_of_its_family() {
        let session = session();
        let (document, model) = session.kernel().snapshot();
        let small = Params::new().with("width", 48.0).with("height", 36.0);

        let checked =
            contract::check(session.kernel().context(), &Id::new("render"), &small, &document, &model);

        assert_eq!(checked, Ok(()));
    }

    #[test]
    fn the_answer_says_in_one_line_what_was_drawn() {
        let mut session = session();

        let reply = session.call("render", &Params::new().with("width", 160.0).with("height", 120.0));

        assert!(reply.ok, "{}", reply.text);
        assert_eq!(reply.text, "render iso 160x120 of plate, ball: 4 faces seen, 1 px = 0.608898\n");
        assert_eq!(reply.exact, Exactness::Sampled { samples: 160 * 120 });
    }

    #[test]
    fn the_picture_comes_as_base64_text_of_a_png_file_of_the_size_asked_for() {
        let mut session = session();

        let reply = session.call("render", &Params::new().with("width", 40.0).with("height", 30.0));

        // The signature, the header, one stored block of 30 lines of 121 bytes, and the end.
        let file_size = 8 + 25 + (12 + 2 + 5 + 30 * 121 + 4) + 12;
        let Value::Text(png) = entry(&reply.data, "png") else { panic!("the picture is not text") };
        assert!(png.starts_with("iVBORw0KGgo"), "{}", &png[..16]);
        assert_eq!(png.len(), usize::div_ceil(file_size, 3) * 4);
        assert_eq!(
            (entry(&reply.data, "width"), entry(&reply.data, "height")),
            (&Value::from(40.0), &Value::from(30.0))
        );
    }

    #[test]
    fn paint_naming_what_is_not_drawn_or_no_colour_is_refused_with_the_reason() {
        let mut session = session();
        let paint = |key: &str, colour: &str| {
            let mut entries = std::collections::BTreeMap::new();
            entries.insert(key.to_string(), Value::from(colour));
            Params::new().with("width", 20.0).with("height", 20.0).with("paint", Value::Map(entries))
        };

        let fine = session.call("render", &paint("plate#t*", "#40c060"));
        let stray = session.call("render", &paint("bridge", "#40c060"));
        let faceless = session.call("render", &paint("plate#wing", "#40c060"));
        let colourless = session.call("render", &paint("plate", "reddish"));

        assert!(fine.ok, "{}", fine.text);
        assert!(stray.text.contains("`bridge`, which is not drawn"), "{}", stray.text);
        assert!(faceless.text.contains("has no face `wing`"), "{}", faceless.text);
        assert!(colourless.text.contains("`reddish` is not a colour"), "{}", colourless.text);
    }

    #[test]
    fn the_default_picture_is_480_by_360_from_the_iso_view() {
        let mut session = session();

        let reply = session.call("render", &Params::new().with("nodes", "ball"));

        assert!(reply.text.starts_with("render iso 480x360 of ball: 1 face seen"), "{}", reply.text);
    }

    #[test]
    fn a_view_is_a_name_or_a_direction_of_sight() {
        let mut session = session();
        let look = |view: Value| Params::new().with("view", view).with("width", 40.0).with("height", 30.0);

        let named = session.call("render", &look(Value::from("top")));
        let aimed = session.call("render", &look(Value::from([0.0, 0.0, -2.0])));
        let unknown = session.call("render", &look(Value::from("sideways")));
        let mistyped = session.call("render", &look(Value::from(true)));

        assert!(named.text.starts_with("render top 40x30 of plate, ball: "), "{}", named.text);
        assert!(aimed.text.starts_with("render toward -z 40x30 of plate, ball: "), "{}", aimed.text);
        assert_eq!(entry(&named.data, "png"), entry(&aimed.data, "png"));
        assert_eq!(
            unknown.text,
            "input: there is no view `sideways`; the views are iso, front, back, left, right, top, bottom, or a direction of sight as x,y,z"
        );
        assert_eq!(
            mistyped.text,
            "input: `view` is the name of a view, a direction of sight as x,y,z, or a camera {eye, toward or target, up, focal, centre}"
        );
    }

    #[test]
    fn what_cannot_be_drawn_is_refused_with_the_reason() {
        let mut session = session();
        let mut empty = {
            let mut builder = crate::testing::builder();
            builder.accuracy(krly3d_core::Accuracy::for_scale(100.0).unwrap());
            builder.session()
        };

        let missing = session.call("render", &Params::new().with("nodes", "lid"));
        let huge = session.call("render", &Params::new().with("width", 100_000.0));
        let nowhere = session.call("render", &Params::new().with("view", [0.0, 0.0, 0.0]));
        let nothing = empty.call("render", &Params::new());

        assert_eq!(missing.text, "not found: there is no node `lid`");
        assert_eq!(huge.text, "input: `width` may be at most 2048 pixels, got 100000");
        assert_eq!(nowhere.text, "input: the direction of sight needs a direction, got no length");
        assert_eq!(nothing.text, "not found: the model has no node to draw");
    }
}
