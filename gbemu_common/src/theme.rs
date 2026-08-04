extern crate alloc;
use alloc::borrow::Cow;
use databake::Bake;
use palette::{Srgb, Srgba};
use serde::{Deserialize, Serialize, de::Visitor};
use std::str::FromStr;

#[cfg(feature = "gpui")]
use gpui::{Hsla, Rgba, rgba};
use palette::stimulus::IntoStimulus;

#[derive(Bake, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[databake(path = gbemu_common::theme)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    /// Name of the author.
    pub author: Cow<'static, str>,

    /// Name of the scheme.
    pub name: Cow<'static, str>,

    /// Variant of the theme
    #[serde(default)]
    pub variant: Variant,

    pub palette: Palette,
}

impl Theme {
    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn variant(&self) -> &Variant {
        &self.variant
    }
}

#[derive(Bake, Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[databake(path = gbemu_common::theme)]
pub enum Variant {
    #[default]
    Dark,
    Light,
    #[serde(untagged)]
    Other(Cow<'static, str>),
}

struct ColorVisitor;

#[derive(Debug, Default, Bake, Clone, Copy, PartialEq, Eq)]
#[databake(path = gbemu_common::theme)]
pub struct Color(pub u32);

impl Visitor<'_> for ColorVisitor {
    type Value = Color;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string in the format #rrggbb or #rrggbbaa, # prefix is optional")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        palette::Srgba::from_str(v)
            .or_else(|_| Srgb::from_str(v).map(|x| x.into()))
            .map_err(E::custom)
            .map(Color::from)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Color, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(ColorVisitor)
    }
}

impl From<Color> for u32 {
    fn from(color: Color) -> u32 {
        color.0
    }
}

impl From<u32> for Color {
    fn from(color: u32) -> Color {
        Color(color)
    }
}

impl From<Color> for palette::Srgba<u8> {
    fn from(color: Color) -> Self {
        color.0.into()
    }
}

impl<T: IntoStimulus<u8>> From<Srgba<T>> for Color {
    fn from(color: Srgba<T>) -> Self {
        Color(color.into_format().into())
    }
}

cfg_select! {
    feature = "gpui" => {
        impl From<Color> for Rgba {
            fn from(value: Color) -> Self {
                rgba(value.0)
            }
        }

        impl From<Color> for Hsla {
            fn from(value: Color) -> Self {
                Rgba::from(value).into()
            }
        }

        impl From<Color> for gpui::Fill {
            fn from(value: Color) -> Self {
                Rgba::from(value).into()
            }
        }

        impl From<Rgba> for Color {
            fn from(value: Rgba) -> Self {
                Color(value.into())
            }
        }
    }
    _ => {}
}

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Srgba::serialize(&Srgba::from(*self), serializer)
    }
}

#[derive(Bake, Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[databake(path = gbemu_common::theme)]
pub struct Palette {
    /// Background. Default Background
    pub base00: Color,

    /// Black. Lighter Background(Used for status bars)
    pub base01: Color,

    /// Bright Black. Selection Background
    pub base02: Color,

    /// Comments, Invisibles, Line Highlighting.
    pub base03: Color,

    /// Dark Foreground (Used for status bars).
    pub base04: Color,

    /// Foreground. Default Foreground, Caret, Delimiters, Operators
    pub base05: Color,

    /// White. Light Foreground (Not often used)
    pub base06: Color,

    /// Bright White. Lightest Foreground (Not often used)
    pub base07: Color,

    /// Red. Variables, XML Tags, Markup Link Text, Markup Lists, Diff Deleted
    pub base08: Color,

    /// Yellow. Integers, Boolean, Constants, XML Attributes, Markup Link Url
    pub base09: Color,

    /// Classes, Markup Bold, Search Text Background.
    #[serde(rename = "base0A")]
    pub base0a: Color,

    /// Green. Colors, Inherited Class, Markup Code, Diff Inserted
    #[serde(rename = "base0B")]
    pub base0b: Color,

    /// Cyan. Support, Regular Expressions, Escape Characters, Markup Quotes
    #[serde(rename = "base0C")]
    pub base0c: Color,

    /// Blue. Functions, Methods, Attribute IDs, Headings
    #[serde(rename = "base0D")]
    pub base0d: Color,

    /// Purple. Keywords, Storage, Selector, Markup Italic, Diff Changed
    #[serde(rename = "base0E")]
    pub base0e: Color,

    /// Deprecated.
    #[serde(rename = "base0F")]
    pub base0f: Color,

    /// Darker Background.
    pub base10: Color,

    /// Darkest Background.
    pub base11: Color,

    /// Bright Red.
    pub base12: Color,

    /// Bright Yellow.
    pub base13: Color,

    /// Bright Green.
    pub base14: Color,

    /// Bright Cyan.
    pub base15: Color,

    /// Bright Blue.
    pub base16: Color,

    /// Bright Purple.
    pub base17: Color,
}

impl Palette {
    pub fn black(&self) -> Color {
        self.base00
    }

    pub fn background(&self) -> Color {
        self.base00
    }

    pub fn darkest_gray(&self) -> Color {
        self.base01
    }

    pub fn lighter_background(&self) -> Color {
        self.base01
    }

    pub fn dark_gray(&self) -> Color {
        self.base02
    }

    pub fn gray(&self) -> Color {
        self.base03
    }
    pub fn bright_black(&self) -> Color {
        self.base03
    }

    pub fn light_gray(&self) -> Color {
        self.base04
    }
    pub fn dark_foreground(&self) -> Color {
        self.base04
    }

    pub fn white(&self) -> Color {
        self.base05
    }
    pub fn foreground(&self) -> Color {
        self.base05
    }

    pub fn lighter_white(&self) -> Color {
        self.base06
    }

    pub fn light_foreground(&self) -> Color {
        self.base06
    }

    pub fn bright_white(&self) -> Color {
        self.base07
    }

    pub fn lightest_foreground(&self) -> Color {
        self.base07
    }

    pub fn red(&self) -> Color {
        self.base08
    }

    pub fn orange(&self) -> Color {
        self.base09
    }

    pub fn yellow(&self) -> Color {
        self.base0a
    }

    pub fn green(&self) -> Color {
        self.base0b
    }

    pub fn cyan(&self) -> Color {
        self.base0c
    }

    pub fn blue(&self) -> Color {
        self.base0d
    }

    pub fn magenta(&self) -> Color {
        self.base0e
    }

    pub fn dark_red(&self) -> Color {
        self.base0f
    }

    pub fn brown(&self) -> Color {
        self.base0f
    }

    pub fn darker_black(&self) -> Color {
        self.base10
    }

    pub fn darker_background(&self) -> Color {
        self.base10
    }

    pub fn darkest_background(&self) -> Color {
        self.base11
    }

    pub fn bright_red(&self) -> Color {
        self.base12
    }

    pub fn bright_yellow(&self) -> Color {
        self.base13
    }

    pub fn bright_green(&self) -> Color {
        self.base14
    }

    pub fn bright_cyan(&self) -> Color {
        self.base15
    }

    pub fn bright_blue(&self) -> Color {
        self.base16
    }

    pub fn bright_magenta(&self) -> Color {
        self.base17
    }
}
