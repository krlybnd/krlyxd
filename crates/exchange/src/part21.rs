//! An ISO 10303-21 exchange file under construction: numbered instances and how values
//! are spelled in them.
//!
//! Instances are numbered in the order they are first written, so the same model always
//! gives the same file. A definition asked for through `shared` is written once and
//! referenced from then on; one asked for through `own` is always a new instance, which
//! is what keeps two vertices at the same point two vertices.

use krly3d_core::math::{Point3, Vec3};
use krly3d_core::Error;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;

/// Reference to an instance, written `#n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ref(usize);

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Default)]
pub(crate) struct Part21 {
    records: Vec<String>,
    known: BTreeMap<String, Ref>,
}

impl Part21 {
    /// A new instance, whatever was written before.
    pub(crate) fn own(&mut self, record: String) -> Ref {
        self.records.push(record);
        Ref(self.records.len())
    }

    /// The instance with this content, written now unless it was written already. Records
    /// refer to shared records by number, so equal content means equal all the way down.
    pub(crate) fn shared(&mut self, record: String) -> Ref {
        if let Some(known) = self.known.get(&record) {
            return *known;
        }
        let written = self.own(record.clone());
        self.known.insert(record, written);
        written
    }

    pub(crate) fn point(&mut self, point: &Point3) -> Result<Ref, Error> {
        let coordinates = triple(&point.coords)?;
        Ok(self.shared(format!("CARTESIAN_POINT('',{coordinates})")))
    }

    /// A direction as a unit vector, so that every instance pointing the same way is the
    /// same instance whatever length the kernel kept.
    pub(crate) fn direction(&mut self, vector: &Vec3) -> Result<Ref, Error> {
        let length = vector.norm();
        if !(length.is_finite() && length > 0.0) {
            return Err(Error::geometry(format!("{vector:?} has no direction to write")));
        }
        let ratios = triple(&(vector / length))?;
        Ok(self.shared(format!("DIRECTION('',{ratios})")))
    }

    /// An `AXIS2_PLACEMENT_3D`: a frame whose z axis is `axis` and whose x axis is
    /// `reference`; its y axis is implied as z × x, which makes it right-handed like the
    /// frames of the kernel.
    pub(crate) fn placement(&mut self, origin: &Point3, axis: &Vec3, reference: &Vec3) -> Result<Ref, Error> {
        let (location, axis, reference) =
            (self.point(origin)?, self.direction(axis)?, self.direction(reference)?);
        Ok(self.shared(format!("AXIS2_PLACEMENT_3D('',{location},{axis},{reference})")))
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// The whole exchange file. `name` goes into the header; the schema is AP214.
    pub(crate) fn file(&self, name: &str) -> String {
        let mut text = String::from("ISO-10303-21;\nHEADER;\n");
        let _ = writeln!(text, "FILE_DESCRIPTION(('krly3d boundary representation'),'2;1');");
        // The time stamp is left empty: a query is a pure function of the model and the
        // kernel has no wall clock, so the same model gives the same file byte for byte.
        let _ = writeln!(
            text,
            "FILE_NAME({},'',(''),(''),{},'krly3d','');",
            string(name),
            string(concat!("krly3d ", env!("CARGO_PKG_VERSION")))
        );
        let _ = writeln!(text, "FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));");
        text.push_str("ENDSEC;\nDATA;\n");
        for (index, record) in self.records.iter().enumerate() {
            let _ = writeln!(text, "#{} = {record};", index + 1);
        }
        text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        text
    }
}

/// A real number: digits with a decimal point and an optional exponent, the shortest
/// spelling that reads back as the same number, so nothing is lost on the way.
pub(crate) fn real(value: f64) -> Result<String, Error> {
    if !value.is_finite() {
        return Err(Error::geometry(format!("{value} cannot be written as a number")));
    }
    // Adding zero turns −0 into 0, which would otherwise make two spellings of one number.
    let spelled = format!("{:?}", value + 0.0);
    Ok(match spelled.split_once('e') {
        Some((mantissa, exponent)) if mantissa.contains('.') => format!("{mantissa}E{exponent}"),
        Some((mantissa, exponent)) => format!("{mantissa}.E{exponent}"),
        None => spelled,
    })
}

/// A text in quotes. Apostrophes and backslashes are doubled; whatever is not printable
/// ASCII is written as its code point in the `\X2\` or `\X4\` form.
pub(crate) fn string(text: &str) -> String {
    let mut quoted = String::from("'");
    for character in text.chars() {
        match character {
            '\'' => quoted.push_str("''"),
            '\\' => quoted.push_str("\\\\"),
            ' '..='~' => quoted.push(character),
            other if (other as u32) <= 0xFFFF => {
                let _ = write!(quoted, "\\X2\\{:04X}\\X0\\", other as u32);
            }
            other => {
                let _ = write!(quoted, "\\X4\\{:08X}\\X0\\", other as u32);
            }
        }
    }
    quoted.push('\'');
    quoted
}

/// `.T.` or `.F.`.
pub(crate) fn logical(flag: bool) -> &'static str {
    if flag {
        ".T."
    } else {
        ".F."
    }
}

/// A list of references: `(#1,#2)`.
pub(crate) fn list(items: &[Ref]) -> String {
    let items: Vec<String> = items.iter().map(Ref::to_string).collect();
    format!("({})", items.join(","))
}

fn triple(vector: &Vec3) -> Result<String, Error> {
    Ok(format!("({},{},{})", real(vector.x)?, real(vector.y)?, real(vector.z)?))
}

#[cfg(test)]
mod tests {
    use super::{list, logical, real, string, Part21};
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::ErrorKind;

    #[test]
    fn a_real_always_has_a_decimal_point_and_reads_back_as_the_same_number() {
        let spelled =
            [1.0, -2.5, 0.1 + 0.2, 1e-7, 1.5e-7, 2.5e21, -0.0].map(|value: f64| real(value).unwrap());

        assert_eq!(spelled, ["1.0", "-2.5", "0.30000000000000004", "1.E-7", "1.5E-7", "2.5E21", "0.0"]);
        let back: Vec<f64> = spelled.iter().map(|text| text.replace(".E", ".0E").parse().unwrap()).collect();
        assert_eq!(back, [1.0, -2.5, 0.1 + 0.2, 1e-7, 1.5e-7, 2.5e21, 0.0]);
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused() {
        let error = real(f64::NAN).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Geometry);
    }

    #[test]
    fn a_text_is_quoted_with_apostrophes_doubled_and_other_scripts_as_code_points() {
        let quoted = [string("front|top"), string("it's"), string("a\\b"), string("tető"), string("𝄞")];

        assert_eq!(
            quoted,
            ["'front|top'", "'it''s'", "'a\\\\b'", "'tet\\X2\\0151\\X0\\'", "'\\X4\\0001D11E\\X0\\'"]
        );
    }

    #[test]
    fn a_shared_definition_is_written_once_and_an_own_one_every_time() {
        let mut file = Part21::default();

        let (first, second) = (file.shared("A()".into()), file.shared("A()".into()));
        let (third, fourth) = (file.own("B()".into()), file.own("B()".into()));

        assert_eq!((first, file.len()), (second, 3));
        assert_ne!(third, fourth);
    }

    #[test]
    fn directions_that_differ_only_in_length_are_one_instance() {
        let mut file = Part21::default();

        let (short, long) = (
            file.direction(&Vec3::new(0.0, 0.0, 1.0)).unwrap(),
            file.direction(&Vec3::new(0.0, 0.0, 7.0)).unwrap(),
        );

        assert_eq!(short, long);
        assert_eq!(file.direction(&Vec3::zeros()).unwrap_err().kind, ErrorKind::Geometry);
    }

    #[test]
    fn instances_are_numbered_from_one_in_the_order_written() {
        let mut file = Part21::default();
        let origin = file.point(&Point3::origin()).unwrap();
        let placed = file.placement(&Point3::origin(), &Vec3::z(), &Vec3::x()).unwrap();

        let text = file.file("part");

        assert_eq!((origin.to_string(), placed.to_string()), ("#1".to_string(), "#4".to_string()));
        assert!(
            text.starts_with("ISO-10303-21;\nHEADER;\n") && text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n")
        );
        assert!(text.contains("#1 = CARTESIAN_POINT('',(0.0,0.0,0.0));\n"), "{text}");
        assert!(text.contains("#4 = AXIS2_PLACEMENT_3D('',#1,#2,#3);\n"), "{text}");
        assert_eq!(
            (logical(true), logical(false), list(&[origin, placed])),
            (".T.", ".F.", "(#1,#4)".into())
        );
    }
}
