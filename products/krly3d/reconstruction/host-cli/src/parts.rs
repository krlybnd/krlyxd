//! Bounded JSON admission. Serde owns syntax, exact fields and duplicate keys;
//! the runtime's typed PartSpec owns all part semantics.

use super::output::diag;
use krly3d_reconstruction::sheet::{PartSpec, MAX_PARTS};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePart {
    id: String,
    seed: [f64; 2],
    height: f64,
}

// A derived struct also accepts positional arrays in serde_json. Require a JSON
// object here, then delegate every field rule to the derived WirePart visitor.
struct PartObject(WirePart);

impl<'de> Deserialize<'de> for PartObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = PartObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a part object with id, seed and height")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<PartObject, A::Error> {
                WirePart::deserialize(de::value::MapAccessDeserializer::new(map)).map(PartObject)
            }
        }

        deserializer.deserialize_map(ObjectVisitor)
    }
}

struct Parts(Vec<PartSpec>);

impl<'de> Deserialize<'de> for Parts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PartsVisitor;

        impl<'de> Visitor<'de> for PartsVisitor {
            type Value = Parts;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "an array containing 1..{MAX_PARTS} part objects")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Parts, A::Error> {
                let mut parts = Vec::with_capacity(MAX_PARTS);
                while let Some(PartObject(part)) = sequence.next_element::<PartObject>()? {
                    // Stop at the first excess item; never collect an unbounded Vec.
                    if parts.len() == MAX_PARTS {
                        return Err(de::Error::custom(format!("parts must contain 1..{MAX_PARTS} entries")));
                    }
                    parts.push(PartSpec { id: part.id, seed: part.seed, height: part.height });
                }
                if parts.is_empty() {
                    return Err(de::Error::custom("parts must not be empty"));
                }
                Ok(Parts(parts))
            }
        }

        deserializer.deserialize_seq(PartsVisitor)
    }
}

pub(super) fn read(
    path: &Path,
    tolerance: f64,
    control: &krlyxd::CallControl,
) -> Result<Vec<PartSpec>, String> {
    let bytes = super::io_control::read_file(path, MAX_BYTES, control)?;
    parse(&bytes, tolerance)
}

fn parse(bytes: &[u8], tolerance: f64) -> Result<Vec<PartSpec>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("parts JSON exceeds 1 MiB".into());
    }
    let Parts(parts) =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid parts JSON: {error}"))?;
    let mut ids = BTreeSet::new();
    for part in &parts {
        part.validate(tolerance).map_err(diag)?;
        if !ids.insert(&part.id) {
            return Err(format!("duplicate part id {}", part.id));
        }
    }
    Ok(parts)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{PartSpec, MAX_BYTES, MAX_PARTS};

    fn parse(bytes: &[u8]) -> Result<Vec<PartSpec>, String> {
        super::parse(bytes, 0.05)
    }

    const PART: &str = r#"{"id":"Plate_1-a","seed":[1,2.5],"height":3}"#;

    #[test]
    fn accepts_exact_schema_in_any_key_order_and_caps_part_count() {
        let parts = parse(format!("[{PART}]").as_bytes()).unwrap();
        assert_eq!(parts[0].id, "Plate_1-a");
        assert_eq!(parts[0].seed, [1.0, 2.5]);
        assert!(parse(br#"[{"height":2,"seed":[0,0],"id":"A"}]"#).is_ok());
        for count in [MAX_PARTS, MAX_PARTS + 1] {
            let entries = (0..count).map(|i| PART.replace("Plate_1-a", &format!("P{i}"))).collect::<Vec<_>>();
            assert_eq!(parse(format!("[{}]", entries.join(",")).as_bytes()).is_ok(), count == MAX_PARTS);
        }
    }

    #[test]
    fn rejects_duplicate_keys_even_when_escaped_and_duplicate_ids() {
        for duplicate in [r#""id":"A""#, r#""\u0069d":"A""#, r#""seed":[1,2]"#, r#""height":3"#] {
            assert!(parse(format!("[{{{duplicate},{}]", &PART[1..]).as_bytes()).is_err());
        }
        assert!(parse(format!("[{PART},{PART}]").as_bytes()).is_err());
    }

    #[test]
    fn rejects_non_json_wrong_types_unknown_fields_and_unsafe_ids() {
        for text in ["[]", "{}", "null", "[true]", "[{}]", "[", "", "[1,]", "[] trailing"] {
            assert!(parse(text.as_bytes()).is_err(), "{text}");
        }
        assert!(parse(br#"[["Plate_1-a",[1,2.5],3]]"#).is_err());
        for value in ["true", "false", "null", "NaN", "Infinity", "1e999", "\"3\"", "{}"] {
            for text in
                [PART.replace("[1,2.5]", &format!("[{value},2]")), PART.replace(":3", &format!(":{value}"))]
            {
                assert!(parse(format!("[{text}]").as_bytes()).is_err(), "{text}");
            }
        }
        for id in ["", "1A", "../A", "a/b", "é", &"A".repeat(65)] {
            assert!(parse(format!("[{}]", PART.replace("Plate_1-a", id)).as_bytes()).is_err());
        }
        for seed in ["[]", "[1]", "[1,2,3]", "null", "[-1,2]"] {
            assert!(parse(format!("[{}]", PART.replace("[1,2.5]", seed)).as_bytes()).is_err());
        }
        for key in ["id", "seed", "height"] {
            let mut part: serde_json::Value = serde_json::from_str(PART).unwrap();
            part.as_object_mut().unwrap().remove(key);
            assert!(parse(format!("[{part}]").as_bytes()).is_err());
        }
        assert!(parse(format!("[{},]", PART).as_bytes()).is_err());
        assert!(parse(format!("[{}] true", PART).as_bytes()).is_err());
        assert!(parse(br#"[{"id":"A","seed":[1,2],"height":3,"extra":0}]"#).is_err());
        assert!(parse(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(super::parse(format!("[{PART}]").as_bytes(), f64::NAN).is_err());
    }
}
