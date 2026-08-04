extern crate alloc;
use alloc::borrow::Cow;
use better_default::Default;
use bytemuck::{Pod, Zeroable};
use databake::Bake;
use palette::{
    FromColor, Hsla, IntoColor, Srgb, Srgba, convert::FromColorUnclamped, rgb::PackedRgba,
};
use serde::{Deserialize, Serialize, de::Visitor};
use std::str::FromStr;

#[cfg(feature = "gpui")]
use gpui::{Hsla, Rgba, rgba};
use palette::stimulus::IntoStimulus;

#[derive(Bake, Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct Color(pub Srgba);

impl core::ops::DerefMut for Color {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl core::ops::Deref for Color {
    type Target = Srgba;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl databake::Bake for Color {
    fn bake(&self, env: &databake::CrateEnv) -> databake::TokenStream {
        env.insert("gbemu_common");
        let color = u32::from(*self).bake(env);
        databake::quote! {
            gbemu_common::theme::Color::from(#color)
        }
    }
}

impl databake::BakeSize for Color {
    fn borrows_size(&self) -> usize {
        0
    }
}

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
            .map(Srgba::from)
            .map(Color)
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
        <Srgba<u8>>::from(color.0).into_u32::<palette::rgb::channels::Rgba>()
    }
}

impl From<u32> for Color {
    fn from(value: u32) -> Color {
        Color(<Srgba<u8>>::from(value).into_format())
    }
}

impl<T> FromColor<T> for Color
where
    T: IntoColor<Srgba>,
{
    fn from_color(value: T) -> Self {
        Self(value.into_color())
    }
}

impl<T> From<Srgba<T>> for Color
where
    T: IntoStimulus<f32>,
{
    fn from(value: Srgba<T>) -> Self {
        Self(value.into_format())
    }
}

impl From<Color> for Srgba {
    fn from(value: Color) -> Self {
        value.0
    }
}

impl From<Color> for Hsla {
    fn from(t: Color) -> Self {
        t.0.into_color()
    }
}

impl FromColor<Color> for Hsla {
    fn from_color(t: Color) -> Self {
        t.0.into_color()
    }
}

impl FromColor<Color> for Srgba {
    fn from_color(t: Color) -> Self {
        t.0.into_color()
    }
}

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Srgba::<u8>::serialize(&self.0.into(), serializer)
    }
}

#[derive(Bake, Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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
