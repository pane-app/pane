//! The arithmetic the colour conversions need, hand-written because the
//! calculator links no math library: it is `no_std`, and `f64`'s
//! transcendental functions belong to the standard library. `sqrt` and
//! `powi` are core's, so they are used as they are.
//!
//! Each function is Newton's iteration or a Taylor series reduced to a
//! small interval, accurate to well past the digits a colour is shown
//! with: a channel is rounded to a whole number and a hue to a degree.

/// The `n`th root of `x`, for `x > 0`: Newton's iteration from the power
/// of two the exponent of `x` names, so it starts within a factor of two
/// of the root and converges quadratically.
pub(super) fn root(x: f64, n: u32) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    // `x` is `m · 2^e` with `m` in [1, 2), so the root is a power of two
    // times the root of `m`, and 2^floor(e/n) is within a factor of two.
    let exponent = ((x.to_bits() >> 52) & 0x7ff) as i64 - 1023;
    let scale = exponent.div_euclid(i64::from(n)).clamp(-1022, 1023);
    let mut y = f64::from_bits((u64::try_from(scale + 1023).unwrap_or(0)) << 52);
    let nth = f64::from(n);
    for _ in 0..80 {
        let next = ((nth - 1.0) * y + x / y.powi(i32::from(n) - 1)) / nth;
        let settled = (next - y).abs() <= 1e-16 * y;
        y = next;
        if settled {
            break;
        }
    }
    y
}

/// The cube root of `x`, any sign.
pub(super) fn cbrt(x: f64) -> f64 {
    if x < 0.0 {
        -root(-x, 3)
    } else {
        root(x, 3)
    }
}

/// `x` to the power 2.4 (a gamma-encoded channel to linear light), for
/// `x >= 0`: the fifth root, to the twelfth.
pub(super) fn linear(x: f64) -> f64 {
    root(x, 5).powi(12)
}

/// `x` to the power 5/12 (linear light to a gamma-encoded channel), for
/// `x >= 0`: the twelfth root, to the fifth.
pub(super) fn encoded(x: f64) -> f64 {
    root(x, 12).powi(5)
}

/// The sine of `radians`, by Taylor series on one quadrant.
pub(super) fn sin(radians: f64) -> f64 {
    let (quadrant, u) = reduced(radians);
    match quadrant {
        0 => sin_of(u),
        1 => cos_of(u),
        2 => -sin_of(u),
        _ => -cos_of(u),
    }
}

/// The cosine of `radians`, by Taylor series on one quadrant.
pub(super) fn cos(radians: f64) -> f64 {
    let (quadrant, u) = reduced(radians);
    match quadrant {
        0 => cos_of(u),
        1 => -sin_of(u),
        2 => -cos_of(u),
        _ => sin_of(u),
    }
}

/// `radians` as a quadrant of the circle and the angle left within it,
/// in [0, π/2).
fn reduced(radians: f64) -> (i32, f64) {
    const TAU: f64 = core::f64::consts::TAU;
    const QUARTER: f64 = core::f64::consts::FRAC_PI_2;
    let turn = radians.rem_euclid(TAU);
    let quadrant = (turn / QUARTER).floor();
    (quadrant as i32, turn - quadrant * QUARTER)
}

/// The sine of `u` in [0, π/2]: Taylor series through u^15, nested
/// inward (Horner's form).
fn sin_of(u: f64) -> f64 {
    let z = u * u;
    let series = 1.0 - z / 210.0;
    let series = 1.0 - z / 156.0 * series;
    let series = 1.0 - z / 110.0 * series;
    let series = 1.0 - z / 72.0 * series;
    let series = 1.0 - z / 42.0 * series;
    let series = 1.0 - z / 20.0 * series;
    u * (1.0 - z / 6.0 * series)
}

/// The cosine of `u` in [0, π/2]: Taylor series through u^14, nested
/// inward (Horner's form).
fn cos_of(u: f64) -> f64 {
    let z = u * u;
    let series = 1.0 - z / 182.0;
    let series = 1.0 - z / 132.0 * series;
    let series = 1.0 - z / 90.0 * series;
    let series = 1.0 - z / 56.0 * series;
    let series = 1.0 - z / 30.0 * series;
    1.0 - z / 2.0 * (1.0 - z / 12.0 * series)
}

/// The angle from the x axis to the point (`y`, `x`), in radians in
/// (−π, π].
pub(super) fn atan2(y: f64, x: f64) -> f64 {
    const PI: f64 = core::f64::consts::PI;
    const QUARTER: f64 = core::f64::consts::FRAC_PI_2;
    if x == 0.0 {
        if y == 0.0 {
            return 0.0;
        }
        return if y > 0.0 { QUARTER } else { -QUARTER };
    }
    let mut angle = atan((y / x).abs());
    if x < 0.0 {
        angle = PI - angle;
    }
    if y < 0.0 {
        angle = -angle;
    }
    angle
}

/// The arctangent of `t >= 0`: the half-angle identity applied until the
/// tangent is small enough for a short Taylor series, then doubled back
/// as many times.
fn atan(t: f64) -> f64 {
    const QUARTER: f64 = core::f64::consts::FRAC_PI_2;
    if t > 1.0 {
        return QUARTER - atan(1.0 / t);
    }
    let mut t = t;
    let mut scale = 1.0;
    while t > 0.01 {
        t /= 1.0 + (1.0 + t * t).sqrt();
        scale *= 2.0;
    }
    let z = t * t;
    scale * (t * (1.0 - z / 3.0 * (1.0 - z / 5.0 * (1.0 - z / 7.0 * (1.0 - z / 9.0)))))
}
