//! The calculator's colours: the forms a query names a colour in, and the
//! forms the answer copies it as (the arithmetic is [`math`]'s).
//!
//! A query names a colour as `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`
//! (hex digits, either case, each short digit standing for two of it), or
//! as a function: `rgb(r, g, b)` and `rgba(r, g, b, a)` with channels
//! 0-255 and an alpha 0-1, `hsl(h, s%, l%)` and `hsla(h, s%, l%, a)` with
//! a hue in degrees, and `oklch(l c h)` with a lightness 0-1, a chroma
//! and a hue in degrees, and an optional `/ a` alpha. The legacy
//! functions' arguments are comma-separated and `oklch`'s
//! space-separated, as CSS writes them; an argument out of range is
//! clamped, as CSS clips it. A number is as the calculator writes it
//! (digits and one decimal point, no sign), and a colour name is not a
//! colour: `red` is a word.
//!
//! The answer is the colour as uppercase hex (`#RRGGBB`, or `#RRGGBBAA`
//! while it is not fully opaque), shown with a swatch, and the Actions
//! panel offers it as hex, RGB, HSL and OKLCH. The OKLab matrices are
//! Björn Ottosson's.

use pane_extension::alloc::format;
use pane_extension::alloc::string::String;
use pane_extension::alloc::vec::Vec;

use super::expression;
use super::math;

/// A colour: sRGB channels 0-255 (rounded when the form it was parsed
/// from does not name whole ones) and an alpha 0-1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Colour {
    red: u8,
    green: u8,
    blue: u8,
    alpha: f64,
}

/// The colour the query names, if it is one.
pub(super) fn parse(query: &str) -> Option<Colour> {
    if let Some(hex) = query.strip_prefix('#') {
        return parse_hex(hex);
    }
    let form = ["rgb(", "rgba(", "hsl(", "hsla(", "oklch("]
        .into_iter()
        .find_map(|form| Some((form, query.strip_prefix(form)?)));
    let (form, rest) = form?;
    let rest = rest.strip_suffix(')')?;
    let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
    match form {
        "rgb(" | "rgba(" => match parts.as_slice() {
            [red, green, blue] => Some(Colour {
                red: byte(number(red)?),
                green: byte(number(green)?),
                blue: byte(number(blue)?),
                alpha: 1.0,
            }),
            [red, green, blue, alpha] => Some(Colour {
                red: byte(number(red)?),
                green: byte(number(green)?),
                blue: byte(number(blue)?),
                alpha: number(alpha)?.clamp(0.0, 1.0),
            }),
            _ => None,
        },
        "hsl(" | "hsla(" => match parts.as_slice() {
            [hue, saturation, lightness] => Some(from_hsl(
                number(hue)?,
                percent(saturation)?,
                percent(lightness)?,
                1.0,
            )),
            [hue, saturation, lightness, alpha] => Some(from_hsl(
                number(hue)?,
                percent(saturation)?,
                percent(lightness)?,
                number(alpha)?.clamp(0.0, 1.0),
            )),
            _ => None,
        },
        // `oklch` separates its arguments with spaces, not commas.
        _ if rest.contains(',') => None,
        _ => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            match parts.as_slice() {
                [lightness, chroma, hue] => Some(from_oklch(
                    number(lightness)?.clamp(0.0, 1.0),
                    number(chroma)?.max(0.0),
                    number(hue)?,
                    1.0,
                )),
                [lightness, chroma, hue, slash, alpha] if *slash == "/" => Some(from_oklch(
                    number(lightness)?.clamp(0.0, 1.0),
                    number(chroma)?.max(0.0),
                    number(hue)?,
                    number(alpha)?.clamp(0.0, 1.0),
                )),
                _ => None,
            }
        }
    }
}

/// `hex`, the digits after `#`, as a colour; `None` unless it holds 3, 4,
/// 6 or 8 hex digits.
fn parse_hex(hex: &str) -> Option<Colour> {
    let digits: Vec<u8> = hex.bytes().map(hex_digit).collect::<Option<Vec<u8>>>()?;
    // Each digit of the short forms stands for two of it: '#3aa' is
    // '#33aaaa', '#3aab' '#33aaaabb'.
    let digits: Vec<u8> = match digits.len() {
        3 | 4 => digits.iter().flat_map(|digit| [*digit, *digit]).collect(),
        6 | 8 => digits,
        _ => return None,
    };
    let channel = |digits: &[u8]| digits[0] * 16 + digits[1];
    Some(Colour {
        red: channel(&digits[0..2]),
        green: channel(&digits[2..4]),
        blue: channel(&digits[4..6]),
        alpha: if digits.len() == 8 {
            f64::from(channel(&digits[6..8])) / 255.0
        } else {
            1.0
        },
    })
}

/// One hex digit, either case, as its value.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `text` as the calculator writes a number.
fn number(text: &str) -> Option<f64> {
    expression::number(text.trim())
}

/// `text` as the number of a percentage, requiring its `%`.
fn percent(text: &str) -> Option<f64> {
    number(text.strip_suffix('%')?)
}

/// `value` as a channel, clamped to 0-255 and rounded.
fn byte(value: f64) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

/// `value` as an alpha's byte.
fn alpha_byte(alpha: f64) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `hue` in degrees, wrapped into [0, 360).
fn degrees(hue: f64) -> f64 {
    hue.rem_euclid(360.0)
}

/// The colour of the HSL values (hue in degrees, saturation and
/// lightness as percentages).
fn from_hsl(hue: f64, saturation: f64, lightness: f64, alpha: f64) -> Colour {
    let (saturation, lightness) = (
        saturation.clamp(0.0, 100.0) / 100.0,
        lightness.clamp(0.0, 100.0) / 100.0,
    );
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = degrees(hue) / 60.0;
    let secondary = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let [red, green, blue] = match sector as u32 {
        0 => [chroma, secondary, 0.0],
        1 => [secondary, chroma, 0.0],
        2 => [0.0, chroma, secondary],
        3 => [0.0, secondary, chroma],
        4 => [secondary, 0.0, chroma],
        _ => [chroma, 0.0, secondary],
    };
    let lift = lightness - chroma / 2.0;
    Colour {
        red: byte((red + lift) * 255.0),
        green: byte((green + lift) * 255.0),
        blue: byte((blue + lift) * 255.0),
        alpha,
    }
}

/// The colour of the OKLCH values (lightness 0-1, chroma, hue in
/// degrees), clipped to sRGB's gamut.
fn from_oklch(lightness: f64, chroma: f64, hue: f64, alpha: f64) -> Colour {
    let radians = degrees(hue).to_radians();
    let (a, b) = (chroma * math::cos(radians), chroma * math::sin(radians));
    // OKLab to linear sRGB, through the cone-like LMS response.
    let long = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let medium = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let short = (lightness - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let channel = |value: f64| {
        // Linear light to a gamma-encoded channel, clipped to [0, 1]:
        // a value outside sRGB's gamut becomes its nearest channel.
        let encoded = if value <= 0.0 {
            0.0
        } else {
            1.055 * math::encoded(value) - 0.055
        };
        byte(encoded * 255.0)
    };
    Colour {
        red: channel(4.0767416621 * long - 3.3077115913 * medium + 0.2309699292 * short),
        green: channel(-1.2684380046 * long + 2.6097574011 * medium - 0.3413993965 * short),
        blue: channel(-0.0041960863 * long - 0.7034186147 * medium + 1.7076147010 * short),
        alpha,
    }
}

impl Colour {
    /// The colour as uppercase hex: `#RRGGBB` while it is fully opaque,
    /// `#RRGGBBAA` otherwise.
    pub(super) fn hex(self) -> String {
        let alpha = alpha_byte(self.alpha);
        if alpha == 255 {
            format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                self.red, self.green, self.blue, alpha
            )
        }
    }

    /// The colour as CSS's `rgb()`.
    pub(super) fn rgb(self) -> String {
        format!("rgb({}, {}, {})", self.red, self.green, self.blue)
    }

    /// The colour as CSS's `hsl()` (the alpha is only in the hex form).
    pub(super) fn hsl(self) -> String {
        let (red, green, blue) = (
            f64::from(self.red) / 255.0,
            f64::from(self.green) / 255.0,
            f64::from(self.blue) / 255.0,
        );
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        let lightness = (max + min) / 2.0;
        let (hue, saturation) = if max == min {
            (0.0, 0.0)
        } else {
            let difference = max - min;
            let saturation = difference / (1.0 - (2.0 * lightness - 1.0).abs());
            let hue = if max == red {
                (green - blue) / difference * 60.0
            } else if max == green {
                ((blue - red) / difference + 2.0) * 60.0
            } else {
                ((red - green) / difference + 4.0) * 60.0
            };
            (hue, saturation)
        };
        format!(
            "hsl({}, {}%, {}%)",
            degrees(hue).round() as i64,
            (saturation * 100.0).round() as i64,
            (lightness * 100.0).round() as i64,
        )
    }

    /// The colour as CSS's `oklch()`: lightness and chroma with three
    /// decimals at most, and a hue in degrees — none of it when the
    /// colour has no chroma.
    pub(super) fn oklch(self) -> String {
        let channel = |value: f64| {
            // A gamma-encoded channel to linear light.
            if value <= 0.04045 {
                value / 12.92
            } else {
                math::linear((value + 0.055) / 1.055)
            }
        };
        let [red, green, blue] = [
            channel(f64::from(self.red) / 255.0),
            channel(f64::from(self.green) / 255.0),
            channel(f64::from(self.blue) / 255.0),
        ];
        // Linear sRGB to OKLab, through the cone-like LMS response.
        let long = 0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue;
        let medium = 0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue;
        let short = 0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue;
        let (long, medium, short) = (math::cbrt(long), math::cbrt(medium), math::cbrt(short));
        let lightness = 0.2104542553 * long + 0.7936177850 * medium - 0.0040720468 * short;
        let a = 1.9779984951 * long - 2.4285922050 * medium + 0.4505937099 * short;
        let b = 0.0259040371 * long + 0.7827717662 * medium - 0.8086757660 * short;
        let chroma = (a * a + b * b).sqrt();
        let hue = math::atan2(b, a).to_degrees();
        let chroma = rounded(chroma);
        let hue = if chroma == "0" {
            0
        } else {
            degrees(hue).round() as i64
        };
        format!("oklch({} {} {})", rounded(lightness), chroma, hue)
    }
}

/// `value` as a component of an `oklch()` copy: three decimals at most,
/// without trailing zeros.
fn rounded(value: f64) -> String {
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed.into()
    }
}
