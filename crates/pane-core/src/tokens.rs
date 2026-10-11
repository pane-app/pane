//! The UI component set's public token names (ADR 0036, #237): the stable
//! vocabulary a designed tree styles with — **space**, **text style**,
//! **text level**, **radius** and **icon size** — which the pane crate
//! resolves onto its private theme (`ui::tokens`), so the theme can change
//! without renaming a token and a token follows the user's appearance.
//! The **tone** token lives with the icon model ([`crate::icons::Tone`]),
//! which the list tree shares with the designed tree.
//!
//! These are names, not values: pane-core holds no theme, so resolving a
//! token is the host window's work. The names are what the JSON tree
//! (`docs/designed-tree.md`), both SDKs and the renderer agree on, and
//! they are the versioned surface — a token a later minor adds is one a
//! Pane of this minor leaves out, never one it refuses the tree for.

/// A space token: the named distances of the UI component set, following
/// the theme's spacing rhythm with its appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Xs,
    S,
    M,
    L,
    Xl,
    Xxl,
}

impl Space {
    /// Every token with its name in the tree, for reading and writing
    /// them.
    pub const ALL: [(&'static str, Space); 6] = [
        ("xs", Space::Xs),
        ("s", Space::S),
        ("m", Space::M),
        ("l", Space::L),
        ("xl", Space::Xl),
        ("xxl", Space::Xxl),
    ];

    /// The token named `name` in the tree.
    pub fn named(name: &str) -> Option<Space> {
        Space::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, token)| *token)
    }

    /// The token's name in the tree.
    pub fn name(self) -> &'static str {
        Space::ALL
            .iter()
            .find(|(_, token)| *token == self)
            .map_or("m", |(name, _)| *name)
    }
}

/// The style of a text: its size, weight and family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyle {
    Heading,
    Title,
    Body,
    Caption,
    Mono,
    SmallMono,
}

impl TextStyle {
    /// Every style with its name in the tree.
    pub const ALL: [(&'static str, TextStyle); 6] = [
        ("heading", TextStyle::Heading),
        ("title", TextStyle::Title),
        ("body", TextStyle::Body),
        ("caption", TextStyle::Caption),
        ("mono", TextStyle::Mono),
        ("small-mono", TextStyle::SmallMono),
    ];

    /// The style named `name` in the tree.
    pub fn named(name: &str) -> Option<TextStyle> {
        TextStyle::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, style)| *style)
    }

    /// The style's name in the tree.
    pub fn name(self) -> &'static str {
        TextStyle::ALL
            .iter()
            .find(|(_, style)| *style == self)
            .map_or("body", |(name, _)| *name)
    }
}

/// The level of a text: its colour, through the alpha of the text colour,
/// as ADR 0035's own levels are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLevel {
    Primary,
    Secondary,
    Tertiary,
    Quaternary,
}

impl TextLevel {
    /// Every level with its name in the tree.
    pub const ALL: [(&'static str, TextLevel); 4] = [
        ("primary", TextLevel::Primary),
        ("secondary", TextLevel::Secondary),
        ("tertiary", TextLevel::Tertiary),
        ("quaternary", TextLevel::Quaternary),
    ];

    /// The level named `name` in the tree.
    pub fn named(name: &str) -> Option<TextLevel> {
        TextLevel::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, level)| *level)
    }

    /// The level's name in the tree.
    pub fn name(self) -> &'static str {
        TextLevel::ALL
            .iter()
            .find(|(_, level)| *level == self)
            .map_or("primary", |(name, _)| *name)
    }
}

/// A radius token: the named corner roundings of the UI component set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radius {
    S,
    M,
    L,
    /// A fully rounded corner: a pill or a disc.
    Full,
}

impl Radius {
    /// Every token with its name in the tree.
    pub const ALL: [(&'static str, Radius); 4] = [
        ("s", Radius::S),
        ("m", Radius::M),
        ("l", Radius::L),
        ("full", Radius::Full),
    ];

    /// The token named `name` in the tree.
    pub fn named(name: &str) -> Option<Radius> {
        Radius::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, token)| *token)
    }

    /// The token's name in the tree.
    pub fn name(self) -> &'static str {
        Radius::ALL
            .iter()
            .find(|(_, token)| *token == self)
            .map_or("m", |(name, _)| *name)
    }
}

/// An icon size token: the named sizes an icon or an image is drawn at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    S,
    M,
    L,
    Xl,
}

impl IconSize {
    /// Every token with its name in the tree.
    pub const ALL: [(&'static str, IconSize); 4] = [
        ("s", IconSize::S),
        ("m", IconSize::M),
        ("l", IconSize::L),
        ("xl", IconSize::Xl),
    ];

    /// The token named `name` in the tree.
    pub fn named(name: &str) -> Option<IconSize> {
        IconSize::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, token)| *token)
    }

    /// The token's name in the tree.
    pub fn name(self) -> &'static str {
        IconSize::ALL
            .iter()
            .find(|(_, token)| *token == self)
            .map_or("m", |(name, _)| *name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_token_round_trips_through_its_name() {
        for (name, token) in Space::ALL {
            assert_eq!(Space::named(name), Some(token));
            assert_eq!(token.name(), name);
        }
        for (name, token) in TextStyle::ALL {
            assert_eq!(TextStyle::named(name), Some(token));
            assert_eq!(token.name(), name);
        }
        for (name, token) in TextLevel::ALL {
            assert_eq!(TextLevel::named(name), Some(token));
            assert_eq!(token.name(), name);
        }
        for (name, token) in Radius::ALL {
            assert_eq!(Radius::named(name), Some(token));
            assert_eq!(token.name(), name);
        }
        for (name, token) in IconSize::ALL {
            assert_eq!(IconSize::named(name), Some(token));
            assert_eq!(token.name(), name);
        }
        assert_eq!(Space::named("huge"), None);
        assert_eq!(TextStyle::named("fancy"), None);
        assert_eq!(TextLevel::named("dim"), None);
        assert_eq!(Radius::named("xs"), None);
        assert_eq!(IconSize::named("xxl"), None);
    }
}
