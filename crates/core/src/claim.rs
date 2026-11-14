//! The grammar of claims, shared by every kernel: `subject relation value`, where the
//! value may be an amount with a tolerance. What a subject means — a volume, a face name,
//! an outline — is the business of the kernel that judges the claim.
//!
//! ```text
//! volume>=100          an amount against a number
//! faces=+1             a signed amount: a change against the value before
//! volume=-502.65±0.01  the same, with a stated tolerance
//! meets=frame          a subject with a word for its value
//! ```

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Equal,
    Less,
    AtMost,
    Greater,
    AtLeast,
}

impl Relation {
    /// Whether `actual` stands in this relation to `value`, `tolerance` allowed.
    pub fn holds(self, actual: f64, value: f64, tolerance: f64) -> bool {
        match self {
            Self::Equal => (actual - value).abs() <= tolerance,
            Self::Less => actual < value,
            Self::AtMost => actual <= value + tolerance,
            Self::Greater => actual > value,
            Self::AtLeast => actual >= value - tolerance,
        }
    }
}

/// A claim split into its three parts, as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phrase<'a> {
    pub subject: &'a str,
    pub relation: Relation,
    pub value: &'a str,
}

impl<'a> Phrase<'a> {
    /// The parts of `text`, or nothing when it has no relation or an empty side.
    pub fn parse(text: &'a str) -> Option<Self> {
        let at = text.find(['<', '>', '='])?;
        let (subject, rest) = (text[..at].trim(), &text[at..]);
        let (relation, value) = match rest.as_bytes() {
            [b'<', b'=', ..] => (Relation::AtMost, &rest[2..]),
            [b'>', b'=', ..] => (Relation::AtLeast, &rest[2..]),
            [b'<', ..] => (Relation::Less, &rest[1..]),
            [b'>', ..] => (Relation::Greater, &rest[1..]),
            _ => (Relation::Equal, &rest[1..]),
        };
        let value = value.trim();
        (!subject.is_empty() && !value.is_empty()).then_some(Self { subject, relation, value })
    }
}

/// A number claimed, with the tolerance it is met within and whether it is a change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amount {
    pub value: f64,
    pub tolerance: f64,
    /// A number written with a sign states a change against the value before.
    pub change: bool,
}

impl Amount {
    /// The amount in `value`. Without a stated tolerance an equality is met by anything
    /// that rounds to the number as written, and an inequality is strict to the number.
    pub fn parse(value: &str, relation: Relation) -> Option<Self> {
        let (stated, tolerance) = match value.split_once('±').or_else(|| value.split_once("+-")) {
            Some((stated, tolerance)) => (
                stated.trim(),
                Some(tolerance.trim().parse::<f64>().ok().filter(|tolerance| *tolerance >= 0.0)?),
            ),
            None => (value, None),
        };
        let parsed = stated.parse::<f64>().ok().filter(|parsed| parsed.is_finite())?;
        let tolerance =
            tolerance.unwrap_or(if relation == Relation::Equal { shown_precision(stated) } else { 0.0 });
        Some(Self { value: parsed, tolerance, change: stated.starts_with(['+', '-']) })
    }
}

/// Half a unit of the last digit written: the precision the number itself shows.
fn shown_precision(number: &str) -> f64 {
    let (mantissa, exponent) = match number.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap_or(0)),
        None => (number, 0),
    };
    let decimals = mantissa.split_once('.').map_or(0, |(_, decimals)| decimals.len() as i32);
    0.5 * 10.0_f64.powi(exponent - decimals)
}

#[cfg(test)]
mod tests {
    use super::{Amount, Phrase, Relation};

    #[test]
    fn a_claim_splits_into_subject_relation_and_value() {
        let phrase = Phrase::parse("volume >= 100").unwrap();

        assert_eq!((phrase.subject, phrase.relation, phrase.value), ("volume", Relation::AtLeast, "100"));
        assert!(Phrase::parse("volume").is_none() && Phrase::parse("=3").is_none());
    }

    #[test]
    fn an_amount_without_a_tolerance_is_met_by_what_rounds_to_it() {
        let amount = Amount::parse("502.65", Relation::Equal).unwrap();

        assert!(Relation::Equal.holds(502.654, amount.value, amount.tolerance));
        assert!(!Relation::Equal.holds(502.656, amount.value, amount.tolerance));
    }

    #[test]
    fn a_signed_amount_is_a_change_and_a_stated_tolerance_is_kept() {
        let amount = Amount::parse("-3±0.5", Relation::Equal).unwrap();

        assert_eq!((amount.value, amount.tolerance, amount.change), (-3.0, 0.5, true));
    }
}
