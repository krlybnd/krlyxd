//! Numbers, points and directions as compact text, the same wherever an answer is written.

use crate::math::{Point3, Vec3};

/// A number with at most six decimals and no trailing zeros.
pub fn num(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// `(x y z)`.
pub fn point(point: &Point3) -> String {
    format!("({} {} {})", num(point.x), num(point.y), num(point.z))
}

/// `+x`, `-z`, … for a direction along a world axis, else the components of its unit vector.
pub fn direction(vector: &Vec3) -> String {
    let unit = vector.normalize();
    let parts = [num(unit.x), num(unit.y), num(unit.z)];
    let along = parts.iter().position(|part| part == "1" || part == "-1");
    match along {
        Some(axis) if parts.iter().filter(|part| *part == "0").count() == 2 => {
            let sign = if parts[axis] == "1" { '+' } else { '-' };
            format!("{sign}{}", ["x", "y", "z"][axis])
        }
        _ => format!("({} {} {})", parts[0], parts[1], parts[2]),
    }
}

#[cfg(test)]
mod tests {
    use super::{direction, num, point};
    use crate::math::{Point3, Vec3};

    #[test]
    fn numbers_are_written_without_noise() {
        assert_eq!(
            [num(10.0), num(0.5), num(-0.0), num(1.0 / 3.0), num(-2.5e-9)],
            ["10", "0.5", "0", "0.333333", "0"]
        );
    }

    #[test]
    fn a_direction_along_an_axis_is_named_after_it() {
        assert_eq!(direction(&Vec3::new(0.0, 0.0, -7.0)), "-z");
        assert_eq!(direction(&Vec3::new(1.0, 1.0, 0.0)), "(0.707107 0.707107 0)");
    }

    #[test]
    fn a_point_is_three_numbers_in_brackets() {
        assert_eq!(point(&Point3::new(1.0, -2.5, 0.0)), "(1 -2.5 0)");
    }
}
