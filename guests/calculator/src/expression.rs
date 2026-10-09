//! The calculator's expression language, deliberately small:
//!
//! - numbers: `12`, `3.5`, `.5` (decimal point, no thousands separators or
//!   exponents);
//! - `+`, `-`, `*` or `×`, `/` or `÷`, and `^` for a power with a whole-number
//!   exponent, with the usual precedence (`^` first and right to left, then
//!   `*` and `/`, then `+` and `-`, each left to right);
//! - a leading `-` or `+` (`-3`, `2 * -(1 + 1)`), and parentheses;
//! - spaces anywhere between those.
//!
//! A query longer than 256 characters, or nesting parentheses more than 64
//! deep, is not understood either, so that no query can exhaust the
//! calculator's stack.
//!
//! An expression has an answer only if it applies at least one of the binary
//! operators: a number alone, like `42` or `(5)`, is not a calculation.
//! Nothing else is understood: no functions, constants, variables, units or
//! percentages. Arithmetic is IEEE double precision; answers are shown with
//! at most 15 significant digits.

use pane_extension::alloc::{format, string::String, vec::Vec};

/// What the calculator makes of a query.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The expression's value, finite.
    Answer(f64),
    /// The query could become an expression by typing more, like `2 +` or
    /// `(1 + 2`.
    Incomplete,
    /// The query is not an expression, or its value is undefined or too
    /// large, like `hello`, `2 + * 3` or `1 / 0`.
    Invalid,
    /// An expression without any operation, like `42`.
    NotACalculation,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Times,
    Divide,
    Power,
    Open,
    Close,
}

/// Why no value was computed.
enum Failure {
    Incomplete,
    Invalid,
}

/// The longest query evaluated, in characters.
const MAX_LENGTH: usize = 256;

/// The deepest nesting of parentheses evaluated.
const MAX_DEPTH: u32 = 64;

/// Evaluates `query` (see the module documentation for the language).
pub fn evaluate(query: &str) -> Outcome {
    if query.chars().count() > MAX_LENGTH {
        return Outcome::Invalid;
    }
    let Some(tokens) = tokens(query) else {
        return Outcome::Invalid;
    };
    let mut parser = Parser {
        tokens: &tokens,
        next: 0,
        operations: 0,
        depth: 0,
    };
    let value = match parser.sum() {
        Ok(value) if parser.next == tokens.len() => value,
        // A closing parenthesis too many, or two values in a row.
        Ok(_) => return Outcome::Invalid,
        Err(Failure::Incomplete) => return Outcome::Incomplete,
        Err(Failure::Invalid) => return Outcome::Invalid,
    };
    if parser.operations == 0 {
        Outcome::NotACalculation
    } else if value.is_finite() {
        Outcome::Answer(value)
    } else {
        Outcome::Invalid
    }
}

/// `query` as tokens; `None` if it has anything outside the language.
fn tokens(query: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = query.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let token = match c {
            c if c.is_whitespace() => continue,
            '+' => Token::Plus,
            '-' | '\u{2212}' => Token::Minus,
            '*' | '×' => Token::Times,
            '/' | '÷' => Token::Divide,
            '^' => Token::Power,
            '(' => Token::Open,
            ')' => Token::Close,
            '0'..='9' | '.' => {
                let mut end = start + c.len_utf8();
                while let Some(&(at, next)) = chars.peek() {
                    if !(next.is_ascii_digit() || next == '.') {
                        break;
                    }
                    end = at + next.len_utf8();
                    chars.next();
                }
                Token::Number(number(&query[start..end])?)
            }
            _ => return None,
        };
        tokens.push(token);
    }
    Some(tokens)
}

/// The value of a number's digits with at most one decimal point.
fn number(text: &str) -> Option<f64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    if fraction.contains('.') || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    let mut value = 0.0;
    for digit in whole.bytes() {
        value = value * 10.0 + f64::from(digit - b'0');
    }
    // Digits of the fraction as a whole number over a power of ten, so that
    // `0.1` is the nearest double to one tenth.
    let mut numerator = 0.0;
    let mut denominator = 1.0;
    for digit in fraction.bytes() {
        numerator = numerator * 10.0 + f64::from(digit - b'0');
        denominator *= 10.0;
    }
    Some(value + numerator / denominator)
}

/// A recursive descent over the tokens, one method per precedence level.
struct Parser<'a> {
    tokens: &'a [Token],
    next: usize,
    /// How many binary operations were applied.
    operations: u32,
    /// How many parentheses enclose the next token.
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.next).copied()
    }

    fn take(&mut self) -> Option<Token> {
        let token = self.peek();
        self.next += 1;
        token
    }

    /// Terms joined by `+` and `-`.
    fn sum(&mut self) -> Result<f64, Failure> {
        let mut value = self.product()?;
        while let Some(operator @ (Token::Plus | Token::Minus)) = self.peek() {
            self.next += 1;
            let right = self.product()?;
            self.operations += 1;
            value = if operator == Token::Plus {
                value + right
            } else {
                value - right
            };
        }
        Ok(value)
    }

    /// Factors joined by `*` and `/`.
    fn product(&mut self) -> Result<f64, Failure> {
        let mut value = self.signed()?;
        while let Some(operator @ (Token::Times | Token::Divide)) = self.peek() {
            self.next += 1;
            let right = self.signed()?;
            self.operations += 1;
            value = if operator == Token::Times {
                value * right
            } else if right == 0.0 {
                return Err(Failure::Invalid);
            } else {
                value / right
            };
        }
        Ok(value)
    }

    /// A power with any number of leading signs. `-2^2` is `-(2^2)`.
    fn signed(&mut self) -> Result<f64, Failure> {
        let mut negative = false;
        while let Some(sign @ (Token::Minus | Token::Plus)) = self.peek() {
            self.next += 1;
            negative ^= sign == Token::Minus;
        }
        let value = self.power()?;
        Ok(if negative { -value } else { value })
    }

    /// An operand, raised to a power if `^` follows; right to left.
    fn power(&mut self) -> Result<f64, Failure> {
        let base = self.operand()?;
        if self.peek() != Some(Token::Power) {
            return Ok(base);
        }
        self.next += 1;
        let exponent = self.signed()?;
        self.operations += 1;
        whole_power(base, exponent).ok_or(Failure::Invalid)
    }

    /// A number or a parenthesized sum.
    fn operand(&mut self) -> Result<f64, Failure> {
        match self.take() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::Open) => {
                if self.depth == MAX_DEPTH {
                    return Err(Failure::Invalid);
                }
                self.depth += 1;
                let value = self.sum()?;
                self.depth -= 1;
                match self.take() {
                    Some(Token::Close) => Ok(value),
                    None => Err(Failure::Incomplete),
                    Some(_) => Err(Failure::Invalid),
                }
            }
            None => Err(Failure::Incomplete),
            Some(_) => Err(Failure::Invalid),
        }
    }
}

/// `base` to the power `exponent`, a whole number; `None` for another
/// exponent, and for zero to a negative power.
fn whole_power(base: f64, exponent: f64) -> Option<f64> {
    if exponent != (exponent as i64) as f64 || exponent.abs() > 1e6 {
        return None;
    }
    let mut remaining = (exponent as i64).unsigned_abs();
    let mut square = base;
    let mut value = 1.0;
    while remaining > 0 {
        if remaining & 1 == 1 {
            value *= square;
        }
        square *= square;
        remaining >>= 1;
    }
    if exponent < 0.0 {
        if value == 0.0 {
            return None;
        }
        value = 1.0 / value;
    }
    Some(value)
}

/// `value` as the calculator shows it: at most 15 significant digits, no
/// trailing zeros, and scientific notation (`1.5e20`) for values from a
/// quadrillion up or below a millionth.
pub fn format(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude == 0.0 {
        return "0".into();
    }
    if !(1e-6..1e15).contains(&magnitude) {
        let text = format!("{value:.14e}");
        let (mantissa, exponent) = text.split_once('e').expect("LowerExp writes an e");
        return format!("{}e{exponent}", trimmed(mantissa));
    }
    let mut digits = 1;
    let mut bound = 10.0;
    while magnitude >= bound {
        digits += 1;
        bound *= 10.0;
    }
    let decimals = 15usize.saturating_sub(digits).min(10);
    let text = format!("{value:.decimals$}");
    let text = trimmed(&text);
    // -0.0000000000001 rounds to "-0".
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// `number` without trailing zeros after its decimal point, or the point.
fn trimmed(number: &str) -> &str {
    if number.contains('.') {
        number.trim_end_matches('0').trim_end_matches('.')
    } else {
        number
    }
}
