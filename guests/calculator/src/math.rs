//! The arithmetic the colour conversions need, hand-written because the
//! calculator links no math library: it is `no_std`, and `f64`'s
//! transcendental functions, and its `sqrt`, `powi`, `round` and
//! `rem_euclid`, belong to the standard library.
//!
//! Each function is Newton's iteration, exponentiation by squaring or a
//! Taylor series reduced to a small interval, accurate to well past the
//! digits a colour is shown with: a channel is rounded to a whole number
//! and a hue to a degree.

/// `x` raised to the `n`th power, a whole number, by squaring.
pub(super) fn power(x: f64, n: u32) -> f64 {
    let mut value = 1.0;
    let mut square = x;
    let mut remaining = n;
    while remaining > 0 {
        if remaining & 1 == 1 {
            value *= square;
        }
        square *= square;
        remaining >>= 1;
    }
    value
}

/// The square root of `x`, for `x >= 0`: Newton's iteration from the
/// power of two the exponent of `x` names.
pub(super) fn sqrt(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let exponent = ((x.to_bits() >> 52) & 0x7ff) as i64 - 1023;
    let scale = exponent.div_euclid(2).clamp(-1022, 1023);
    let mut y = f64::from_bits((u64::try_from(scale + 1023).unwrap_or(0)) << 52);
    for _ in 0..80 {
        let next = (y + x / y) / 2.0;
        let settled = (next - y).abs() <= 1e-16 * y;
        y = next;
        if settled {
            break;
        }
    }
    y
}

/// The largest whole number not greater than `x`, for a finite `x`:
/// the fraction bits of `|x|` cleared, and a whole negative `x` kept as
/// it is (a fraction steps it down).
pub(super) fn floor(x: f64) -> f64 {
    let whole = truncate(x.abs());
    if x >= 0.0 {
        whole
    } else if whole == x.abs() {
        // Already whole.
        x
    } else {
        -(whole + 1.0)
    }
}

/// `v` without its fraction bits, for a finite `v >= 0` (a whole `v`
/// unchanged).
fn truncate(v: f64) -> f64 {
    let bits = v.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64 - 1023;
    if exponent < 0 {
        // Below one, there is no whole part.
        return 0.0;
    }
    if exponent > 52 {
        // This large, every double is whole.
        return v;
    }
    // How many of the mantissa's 52 bits sit below the value's binary
    // point: the low that many of them are the fraction.
    let fraction = (52 - exponent) as u32;
    f64::from_bits(bits & !((1u64 << fraction) - 1))
}

/// `x` rounded to the nearest whole number, half away from zero.
pub(super) fn round(x: f64) -> f64 {
    if x < 0.0 {
        -round(-x)
    } else {
        floor(x + 0.5)
    }
}

/// The remainder of `x / m`, in [0, m) for `m > 0`.
pub(super) fn remainder(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r < 0.0 {
        r + m
    } else {
        r
    }
}

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
        let next = ((nth - 1.0) * y + x / power(y, n - 1)) / nth;
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
    power(root(x, 5), 12)
}

/// `x` to the power 5/12 (linear light to a gamma-encoded channel), for
/// `x >= 0`: the twelfth root, to the fifth.
pub(super) fn encoded(x: f64) -> f64 {
    power(root(x, 12), 5)
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
    let turn = remainder(radians, TAU);
    let quadrant = floor(turn / QUARTER);
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
        t /= 1.0 + sqrt(1.0 + t * t);
        scale *= 2.0;
    }
    let z = t * t;
    scale * (t * (1.0 - z / 3.0 * (1.0 - z / 5.0 * (1.0 - z / 7.0 * (1.0 - z / 9.0)))))
}
