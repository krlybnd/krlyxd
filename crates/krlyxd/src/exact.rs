//! How far a number can be trusted.

/// Attached to every numeric answer so a reader can tell a closed form from an estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exactness {
    /// Closed form: only floating-point round-off.
    Exact,
    /// Numerical solution, accurate to the stated absolute tolerance.
    Numeric { tolerance: f64 },
    /// A bound obtained from a finite number of samples.
    Sampled { samples: u32 },
}

impl Exactness {
    /// The mark used in text answers: `=` exact, `~` numeric, `?` sampled.
    pub fn symbol(self) -> char {
        match self {
            Self::Exact => '=',
            Self::Numeric { .. } => '~',
            Self::Sampled { .. } => '?',
        }
    }

    /// Exactness of a result computed from two inputs: as trustworthy as its weaker input.
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Sampled { samples: a }, Self::Sampled { samples: b }) => {
                Self::Sampled { samples: a.min(b) }
            }
            (sampled @ Self::Sampled { .. }, _) | (_, sampled @ Self::Sampled { .. }) => sampled,
            (Self::Numeric { tolerance: a }, Self::Numeric { tolerance: b }) => {
                Self::Numeric { tolerance: a.max(b) }
            }
            (numeric @ Self::Numeric { .. }, _) | (_, numeric @ Self::Numeric { .. }) => numeric,
            (Self::Exact, Self::Exact) => Self::Exact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Exactness;

    #[test]
    fn symbols_distinguish_the_three_levels() {
        let symbols = [
            Exactness::Exact.symbol(),
            Exactness::Numeric { tolerance: 1e-9 }.symbol(),
            Exactness::Sampled { samples: 64 }.symbol(),
        ];

        assert_eq!(symbols, ['=', '~', '?']);
    }

    #[test]
    fn combining_keeps_the_weaker_level() {
        let numeric = Exactness::Numeric { tolerance: 1e-9 };
        let sampled = Exactness::Sampled { samples: 64 };

        assert_eq!(Exactness::Exact.and(numeric), numeric);
        assert_eq!(numeric.and(sampled), sampled);
        assert_eq!(Exactness::Exact.and(Exactness::Exact), Exactness::Exact);
    }

    #[test]
    fn combining_two_numeric_values_keeps_the_looser_tolerance() {
        let combined = Exactness::Numeric { tolerance: 1e-9 }.and(Exactness::Numeric { tolerance: 1e-6 });

        assert_eq!(combined, Exactness::Numeric { tolerance: 1e-6 });
    }
}
