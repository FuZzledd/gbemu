use crate::ppu::Mode;
// use nom::{
//     IResult, Parser,
//     branch::alt,
//     bytes::{tag, tag_no_case},
//     character::{char, complete::multispace1},
//     combinator::map_res,
//     error::ParseError,
//     multi::separated_list1,
//     sequence::{delimited, preceded, separated_pair},
// };
use crate::{BREAKPOINTS, GameBoy, PLAYBACK_CONTROLLER, PlaybackMessage};
use better_default::Default;
use chumsky::Parser;
use chumsky::container::OrderedSeq;
use chumsky::extra::Err;
use chumsky::input::InputRef;
use chumsky::prelude::*;
use chumsky::text::digits;
use chumsky::text::{ident, whitespace};
use docstr::docstr;
use gbemu_common::theme::{Color, Palette};
use itertools::Itertools;
use num_traits::Num;
use spire_enum::prelude::{delegate_impl, delegated_enum};
use std::borrow::Borrow;
use std::fmt::{Display, Formatter};
use std::sync::LazyLock;
use std::{collections::HashMap, iter, rc::Rc};
use strum::EnumDiscriminants;

pub type HitBreakpoint = (usize, BreakpointData);

pub static BREAKPOINT_REPORTER: LazyLock<(
    flume::Sender<Vec<HitBreakpoint>>,
    flume::Receiver<Vec<HitBreakpoint>>,
)> = LazyLock::new(|| flume::bounded(32));

#[derive(Debug, Default)]
pub struct Breakpoints {
    pub id_counter: usize,
    pub id_map: HashMap<usize, BreakpointType>,

    pub ppu_mode: HashMap<usize, BreakpointEntry<PpuModeBreakpoint>>,
    pub pc_value: HashMap<usize, BreakpointEntry<PcValueBreakpoint>>,
    pub watch_read: HashMap<usize, BreakpointEntry<WatchReadBreakpoint>>,
    pub watch_write: HashMap<usize, BreakpointEntry<WatchWriteBreakpoint>>,
    pub watch_all: HashMap<usize, BreakpointEntry<WatchAllBreakpoint>>,
}

#[derive(Clone, Copy, Debug)]
pub struct BreakpointEntry<T> {
    pub enabled: bool,
    pub breakpoint: T,
}
impl<T> From<T> for BreakpointEntry<T> {
    fn from(value: T) -> Self {
        BreakpointEntry {
            enabled: true,
            breakpoint: value,
        }
    }
}
impl<T> BreakpointEntry<T> {
    fn map<R>(self, mut func: impl FnMut(T) -> R) -> BreakpointEntry<R> {
        BreakpointEntry {
            enabled: self.enabled,
            breakpoint: func(self.breakpoint),
        }
    }
}

impl<T: Display> Display for BreakpointEntry<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{breakpoint} - {enabled}",
            breakpoint = self.breakpoint,
            enabled = if self.enabled { "enabled" } else { "disabled" }
        )
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum AddBreakpointError {
    #[error("Invalid breakpoint {0:?}")]
    Invalid(Breakpoint),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum AccessBreakpointError {
    #[error("Breakpoint doesn't exist: {0:?}")]
    NonExistent(usize),
}

impl Breakpoints {
    pub fn add_breakpoint(
        &mut self,
        breakpoint: impl Into<Breakpoint>,
    ) -> Result<usize, AddBreakpointError> {
        let id = self.id_counter;

        let breakpoint = breakpoint.into();
        let breakpoint_type = BreakpointType::from(&breakpoint);

        match breakpoint {
            Breakpoint::PpuMode(breakpoint) => {
                self.ppu_mode.insert(id, breakpoint.into());
            }
            Breakpoint::PcValue(breakpoint) => {
                self.pc_value.insert(id, breakpoint.into());
            }
            Breakpoint::WatchRead(breakpoint) => {
                self.watch_read.insert(id, breakpoint.into());
            }
            Breakpoint::WatchWrite(breakpoint) => {
                self.watch_write.insert(id, breakpoint.into());
            }
            Breakpoint::WatchAll(breakpoint) => {
                self.watch_all.insert(id, breakpoint.into());
            }
        }

        self.id_map.insert(id, breakpoint_type);

        self.id_counter += 1;
        Ok(id)
    }

    pub fn remove_breakpoint(
        &mut self,
        id: usize,
    ) -> Result<BreakpointEntry<Breakpoint>, AccessBreakpointError> {
        let breakpoint_type = self
            .id_map
            .remove(&id)
            .ok_or(AccessBreakpointError::NonExistent(id))?;

        match breakpoint_type {
            BreakpointType::PpuMode => self
                .ppu_mode
                .remove(&id)
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::PcValue => self
                .pc_value
                .remove(&id)
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchRead => self
                .watch_read
                .remove(&id)
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchWrite => self
                .watch_write
                .remove(&id)
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchAll => self
                .watch_all
                .remove(&id)
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
        }
    }

    pub fn get_breakpoint(
        &mut self,
        id: usize,
    ) -> Result<BreakpointEntry<Breakpoint>, AccessBreakpointError> {
        let breakpoint_type = self
            .id_map
            .get(&id)
            .ok_or(AccessBreakpointError::NonExistent(id))?;

        match breakpoint_type {
            BreakpointType::PpuMode => self
                .ppu_mode
                .get(&id)
                .cloned()
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::PcValue => self
                .pc_value
                .get(&id)
                .cloned()
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchRead => self
                .watch_read
                .get(&id)
                .cloned()
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchWrite => self
                .watch_write
                .get(&id)
                .cloned()
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
            BreakpointType::WatchAll => self
                .watch_all
                .get(&id)
                .cloned()
                .ok_or(AccessBreakpointError::NonExistent(id))
                .map(|entry| entry.map(Breakpoint::from)),
        }
    }

    pub fn set_breakpoint_enable(
        &mut self,
        id: usize,
        enabled: bool,
    ) -> Result<(), AccessBreakpointError> {
        let breakpoint_type = self
            .id_map
            .get(&id)
            .ok_or(AccessBreakpointError::NonExistent(id))?;

        match breakpoint_type {
            BreakpointType::PpuMode => {
                self.ppu_mode
                    .get_mut(&id)
                    .ok_or(AccessBreakpointError::NonExistent(id))?
                    .enabled = enabled;
            }
            BreakpointType::PcValue => {
                self.pc_value
                    .get_mut(&id)
                    .ok_or(AccessBreakpointError::NonExistent(id))?
                    .enabled = enabled;
            }
            BreakpointType::WatchRead => {
                self.watch_read
                    .get_mut(&id)
                    .ok_or(AccessBreakpointError::NonExistent(id))?
                    .enabled = enabled;
            }
            BreakpointType::WatchWrite => {
                self.watch_write
                    .get_mut(&id)
                    .ok_or(AccessBreakpointError::NonExistent(id))?
                    .enabled = enabled;
            }
            BreakpointType::WatchAll => {
                self.watch_all
                    .get_mut(&id)
                    .ok_or(AccessBreakpointError::NonExistent(id))?
                    .enabled = enabled;
            }
        }
        Ok(())
    }
}

#[derive(Debug, EnumDiscriminants, Clone, Copy)]
#[delegated_enum(impl_conversions)]
#[strum_discriminants(name(BreakpointType))]
pub enum Breakpoint {
    PpuMode(PpuModeBreakpoint),
    PcValue(PcValueBreakpoint),
    WatchRead(WatchReadBreakpoint),
    WatchWrite(WatchWriteBreakpoint),
    WatchAll(WatchAllBreakpoint),
}

#[delegate_impl]
impl Display for Breakpoint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result;
}

#[derive(Debug, Clone, Copy)]
pub struct PpuModeBreakpoint {
    pub mode: Mode,
}

impl Display for PpuModeBreakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mode == {}", self.mode)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PcValueBreakpoint {
    pub pc: u16,
}

impl Display for PcValueBreakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PC == 0x{:04X}", self.pc)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchReadBreakpoint {
    pub addr: u16,
}

impl Display for WatchReadBreakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "read @ 0x{:04X}", self.addr)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchWriteBreakpoint {
    pub addr: u16,
}

impl Display for WatchWriteBreakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "write @ 0x{:04X}", self.addr)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchAllBreakpoint {
    pub addr: u16,
}

impl Display for WatchAllBreakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "read/write @ 0x{:04X}", self.addr)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BreakpointData {
    Addr(u16),
    Write {
        addr: u16,
        written_value: u8,
        prev_value: u8,
        new_value: u8,
    },
    Read {
        addr: u16,
        value: u8,
    },
    PpuMode(Mode),
    None,
}

impl Display for BreakpointData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BreakpointData::Addr(addr) => write!(f, "hit 0x{:04X}", addr),
            BreakpointData::Write {
                addr,
                written_value,
                prev_value,
                new_value,
            } => {
                write!(
                    f,
                    "write @ 0x{:04X}, value: 0x{:02X} (prev: 0x{:02X}, new: 0x{:02X})",
                    addr, written_value, prev_value, new_value
                )
            }
            BreakpointData::Read { addr, value } => {
                write!(f, "read @ 0x{:04X}, value: 0x{:02X}", addr, value)
            }
            BreakpointData::PpuMode(mode) => write!(f, "Mode == {}", mode),
            BreakpointData::None => write!(f, "no condition"),
        }
    }
}

pub type StyledTextOutput = Vec<StyledText>;

#[derive(Clone, Default)]
pub enum StyledText {
    #[default]
    Default(Rc<str>),
    Styled(Styled),
}

impl<T> From<T> for StyledText
where
    T: AsRef<str>,
{
    fn from(value: T) -> Self {
        StyledText::Default(value.as_ref().into())
    }
}

#[macro_export]
macro_rules! text {
    ($($text:expr),* $(,)?) => {
        vec![$($crate::debugging::StyledText::from($text)),*]
    }
}

use crate::context::Memory;
pub use text;

#[derive(Clone, Default)]
pub struct Styled {
    pub text: Rc<str>,
    pub weight: Option<TextWeight>,
    pub style: Option<TextStyle>,
    pub palette_fn: Option<Rc<dyn Fn(Palette) -> Color>>,
}

impl StyledText {
    pub fn new(text: impl AsRef<str>) -> Self {
        Self::Default(text.as_ref().into())
    }

    pub fn with_weight(self, weight: TextWeight) -> Self {
        match self {
            Self::Default(text) => Self::Styled(Styled {
                text,
                weight: Some(weight),
                ..Default::default()
            }),
            Self::Styled(old) => Self::Styled(Styled {
                weight: Some(weight),
                ..old
            }),
        }
    }

    pub fn with_style(self, style: TextStyle) -> Self {
        match self {
            Self::Default(text) => Self::Styled(Styled {
                text,
                style: Some(style),
                ..Default::default()
            }),
            Self::Styled(old) => Self::Styled(Styled {
                style: Some(style),
                ..old
            }),
        }
    }

    pub fn with_color(self, palette_fn: impl Fn(Palette) -> Color + Clone + 'static) -> Self {
        match self {
            Self::Default(text) => Self::Styled(Styled {
                text,
                palette_fn: Some(Rc::new(palette_fn)),
                ..Default::default()
            }),
            Self::Styled(old) => Self::Styled(Styled {
                palette_fn: Some(Rc::new(palette_fn)),
                ..old
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TextWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TextStyle {
    #[default]
    Normal,
    Italics,
}

type ErrorTy<'src> = Err<Rich<'src, char>>;

pub fn command_parser<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    choice((
        parse_help(),
        parse_add_breakpoint(),
        parse_delete_breakpoints(),
        parse_list_breakpoints(),
        parse_enable_breakpoints(),
        parse_disable_breakpoints(),
        parse_continue(),
        parse_tick(),
        parse_frame(),
        parse_dump(),
        parse_write_byte(),
        parse_read_byte(),
    ))
    .padded()
}

pub fn parse_help<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("help")
        .ignore_then(ident().or_not())
        .map(|command| Command::Help(command.map(String::from)))
}

pub fn parse_dump<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("dump").ignored().map(|_| Command::DumpState)
}

pub fn parse_tick<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("tick")
        .or(just_no_case("t"))
        .ignore_then(whitespace().at_least(1).ignore_then(parse_num()).or_not())
        .map(|ticks| Command::Tick(ticks.unwrap_or(1)))
}

pub fn parse_continue<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("continue")
        .or(just_no_case("c"))
        .to(Command::Continue)
}

pub fn parse_frame<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("frame")
        .or(just_no_case("f"))
        .ignore_then(whitespace().at_least(1).ignore_then(parse_num()).or_not())
        .map(|frames| Command::Frame(frames.unwrap_or(1)))
}

pub fn parse_add_breakpoint<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("addb")
        .ignore_then(whitespace().at_least(1))
        .ignore_then(
            parse_breakpoint()
                .separated_by(whitespace().at_least(1))
                .at_least(1)
                .collect(),
        )
        .map(Command::AddBreakpoint)
}

pub fn parse_delete_breakpoints<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("delb")
        .ignore_then(whitespace().at_least(1))
        .ignore_then(
            parse_num()
                .separated_by(whitespace().at_least(1))
                .at_least(1)
                .collect(),
        )
        .map(Command::DeleteBreakpoints)
}

pub fn parse_enable_breakpoints<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("enableb")
        .ignore_then(whitespace().at_least(1))
        .ignore_then(
            parse_num()
                .separated_by(whitespace().at_least(1))
                .at_least(1)
                .collect(),
        )
        .map(Command::EnableBreakpoints)
}

pub fn parse_disable_breakpoints<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("disableb")
        .ignore_then(whitespace().at_least(1))
        .ignore_then(
            parse_num()
                .separated_by(whitespace().at_least(1))
                .at_least(1)
                .collect(),
        )
        .map(Command::DisableBreakpoints)
}

pub fn parse_list_breakpoints<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("listb").ignored().to(Command::ListBreakpoints)
}

pub fn just_no_case<
    'src,
    I: Input<'src, Span = SimpleSpan, Token = char> + Display,
    T: OrderedSeq<'src, I::Token> + Clone + Display,
>(
    pattern: T,
) -> impl Parser<'src, I, T, ErrorTy<'src>>
where
    I::Token: PartialEq,
{
    custom(move |inp: &mut InputRef<'src, '_, I, ErrorTy<'src>>| {
        let seq = pattern.clone();
        for next in seq.seq_iter() {
            let before = inp.save();
            match inp.next_maybe() {
                Some(tok) if next.borrow().eq_ignore_ascii_case(&tok.into_inner()) => {}
                found => {
                    let span = inp.span_since(before.cursor());
                    inp.rewind(before);
                    return Err(Rich::custom(
                        span,
                        format!(
                            "Expected {pattern}, found \"{found}\"",
                            found = found.map(|x| x.into_inner()).unwrap_or(' ')
                        ),
                    ));
                }
            }
        }
        Ok(seq.clone())
    })
}

pub fn parse_read_byte<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("r")
        .ignore_then(
            choice((
                just_no_case("/b").map(|_| Radix::Binary),
                just_no_case("/o").map(|_| Radix::Octal),
                just_no_case("/x").map(|_| Radix::Hexadecimal),
            ))
            .or_not()
            .map(|radix| radix.unwrap_or_default()),
        )
        .then(whitespace().at_least(1).ignore_then(parse_num()))
        .map(|(radix, addr)| Command::ReadByte { radix, addr })
}

pub fn parse_write_byte<'src>() -> impl Parser<'src, &'src str, Command, ErrorTy<'src>> {
    just_no_case("w").ignore_then(
        group((
            choice((
                just_no_case("/b").map(|_| Radix::Binary),
                just_no_case("/o").map(|_| Radix::Octal),
                just_no_case("/x").map(|_| Radix::Hexadecimal),
            ))
            .or_not()
            .map(|radix| radix.unwrap_or_default()),
            whitespace().at_least(1).ignore_then(parse_num()),
            whitespace().at_least(1).ignore_then(parse_num()),
        ))
        .map(|(radix, addr, value)| Command::WriteByte { addr, value, radix }),
    )
}

pub fn parse_breakpoint<'src>() -> impl Parser<'src, &'src str, Breakpoint, ErrorTy<'src>> {
    choice((
        just("r:")
            .ignore_then(parse_num::<u16>())
            .map(|addr| WatchReadBreakpoint { addr }.into()),
        just("w:")
            .ignore_then(parse_num::<u16>())
            .map(|addr| WatchWriteBreakpoint { addr }.into()),
        just("rw:")
            .ignore_then(parse_num::<u16>())
            .map(|addr| WatchAllBreakpoint { addr }.into()),
        just("ppu:")
            .ignore_then(parse_ppu_mode())
            .map(|mode| PpuModeBreakpoint { mode }.into()),
        just("pc:")
            .or_not()
            .ignore_then(parse_num::<u16>())
            .map(|pc| PcValueBreakpoint { pc }.into()),
    ))
}

#[derive(Default, Debug, Copy, Clone)]
pub enum Radix {
    #[default]
    Decimal,
    Binary,
    Octal,
    Hexadecimal,
}

#[derive(EnumDiscriminants, Clone)]
pub enum Command {
    Help(Option<String>),
    AddBreakpoint(Vec<Breakpoint>),
    ListBreakpoints,
    DeleteBreakpoints(Vec<usize>),
    EnableBreakpoints(Vec<usize>),
    DisableBreakpoints(Vec<usize>),
    Frame(usize),
    Tick(usize),
    DumpState,
    ReadByte { radix: Radix, addr: u16 },
    WriteByte { addr: u16, value: u8, radix: Radix },
    Continue,
}

impl Command {
    pub fn handle(self, output: &mut impl Extend<StyledTextOutput>, gameboy: &mut GameBoy) {
        match self {
            Command::Help(command) => Self::handle_help(command, output),
            Command::AddBreakpoint(breakpoints) => {
                let mut global_breakpoints = BREAKPOINTS.lock();
                let (added, errors): (Vec<_>, Vec<_>) = breakpoints
                    .into_iter()
                    .map(|breakpoint| global_breakpoints.add_breakpoint(breakpoint))
                    .partition(Result::is_ok);
                let added = added.into_iter().map(Result::unwrap).collect_vec();
                let errors = errors.into_iter().map(Result::unwrap_err).collect_vec();
                output.extend(
                    iter::once(text![format!(
                        "Added breakpoints: {}",
                        added.into_iter().join(", ")
                    ),])
                    .chain(errors.into_iter().map(|error| {
                        text![
                            "\n",
                            styled("Unable to add breakpoint: ")
                                .with_color(|palette| palette.red()),
                            bold(error.to_string()).with_color(|palette| palette.red())
                        ]
                    })),
                )
            }
            Command::ListBreakpoints => {
                let breakpoints = BREAKPOINTS.lock();

                let mut text_output = vec![];

                text_output.push(text![bold("List of breakpoints:")]);

                if !breakpoints.pc_value.is_empty() {
                    text_output.push(text![bold("PC Breakpoints")]);
                    text_output.extend(
                        breakpoints
                            .pc_value
                            .iter()
                            .map(|(id, breakpoint)| text![format!("{id} - {breakpoint}")]),
                    )
                }

                if !breakpoints.ppu_mode.is_empty() {
                    text_output.push(text![bold("PPU Mode Breakpoints")]);
                    text_output.extend(
                        breakpoints
                            .ppu_mode
                            .iter()
                            .map(|(id, breakpoint)| text![format!("{id} - {breakpoint}")]),
                    )
                }

                if !breakpoints.watch_read.is_empty() {
                    text_output.push(text![bold("Read Watches")]);
                    text_output.extend(
                        breakpoints
                            .watch_read
                            .iter()
                            .map(|(id, breakpoint)| text![format!("{id} - {breakpoint}")]),
                    )
                }

                if !breakpoints.watch_write.is_empty() {
                    text_output.push(text![bold("Write Watches")]);
                    text_output.extend(
                        breakpoints
                            .watch_write
                            .iter()
                            .map(|(id, breakpoint)| text![format!("{id} - {breakpoint}")]),
                    )
                }

                if !breakpoints.watch_all.is_empty() {
                    text_output.push(text![bold("Read/Write Watches")]);
                    text_output.extend(
                        breakpoints
                            .watch_all
                            .iter()
                            .map(|(id, breakpoint)| text![format!("{id} - {breakpoint}")]),
                    )
                }
                output.extend(text_output);
            }
            Command::DeleteBreakpoints(ids) => {
                let mut text_output = vec![];
                let mut successful = vec![];
                let mut unsuccessful = vec![];
                let mut global_breakpoints = BREAKPOINTS.lock();
                for id in ids {
                    match global_breakpoints.remove_breakpoint(id) {
                        Ok(_) => {
                            successful.push(id);
                        }
                        Err(err) => {
                            if let AccessBreakpointError::NonExistent(id) = err {
                                unsuccessful.push(text![
                                    styled(format!("Couldn't find breakpoint {id}"))
                                        .with_color(|palette| palette.red())
                                ]);
                            }
                        }
                    };
                }
                text_output.push(text![format!(
                    "Removed breakpoints: {}",
                    successful.into_iter().join(", ")
                ),]);
                text_output.extend(unsuccessful);
                output.extend(text_output);
            }
            Command::EnableBreakpoints(ids) => {
                let mut text_output = vec![];
                let mut successful = vec![];
                let mut unsuccessful = vec![];
                let mut global_breakpoints = BREAKPOINTS.lock();
                for id in ids {
                    match global_breakpoints.set_breakpoint_enable(id, true) {
                        Ok(_) => {
                            successful.push(id);
                        }
                        Err(err) => {
                            if let AccessBreakpointError::NonExistent(id) = err {
                                unsuccessful.push(text![
                                    styled(format!("Couldn't find breakpoint {id}"))
                                        .with_color(|palette| palette.red())
                                ]);
                            }
                        }
                    };
                }
                text_output.push(text![format!(
                    "Enabled breakpoints: {}",
                    successful.into_iter().join(", ")
                ),]);
                text_output.extend(unsuccessful);
                output.extend(text_output);
            }
            Command::DisableBreakpoints(ids) => {
                let mut text_output = vec![];
                let mut successful = vec![];
                let mut unsuccessful = vec![];
                let mut global_breakpoints = BREAKPOINTS.lock();
                for id in ids {
                    match global_breakpoints.set_breakpoint_enable(id, false) {
                        Ok(_) => {
                            successful.push(id);
                        }
                        Err(err) => {
                            if let AccessBreakpointError::NonExistent(id) = err {
                                unsuccessful.push(text![
                                    styled(format!("Couldn't find breakpoint {id}"))
                                        .with_color(|palette| palette.red())
                                ]);
                            }
                        }
                    };
                }
                text_output.push(text![format!(
                    "Disabled breakpoints: {}",
                    successful.into_iter().join(", ")
                ),]);
                text_output.extend(unsuccessful);
                output.extend(text_output);
            }
            Command::Continue => {
                PLAYBACK_CONTROLLER.0.send(PlaybackMessage::Play).unwrap();
            }
            Command::Frame(frames) => {
                PLAYBACK_CONTROLLER
                    .0
                    .send(PlaybackMessage::StepFrame(frames))
                    .unwrap();
            }
            Command::Tick(ticks) => {
                PLAYBACK_CONTROLLER
                    .0
                    .send(PlaybackMessage::StepTick(ticks))
                    .unwrap();
            }
            Command::DumpState => {
                output.extend([text![gameboy.cpu.dump_state(&mut gameboy.context)]]);
            }
            Command::ReadByte { addr, radix } => {
                let value = gameboy.context.memory.read_u8(addr);

                let formatted = format_radix_u8(radix, value);

                output.extend([text![format!("*0x{addr:04X} == {formatted}")]])
            }
            Command::WriteByte { addr, value, radix } => {
                let old_value = format_radix_u8(radix, gameboy.context.memory.read_u8(addr));
                gameboy.context.memory.write_u8(addr, value);
                let new_value = format_radix_u8(radix, gameboy.context.memory.read_u8(addr));

                let written = format_radix_u8(radix, value);
                let addr = format_radix_u16(Radix::Hexadecimal, addr);

                output.extend([text![format!(
                    "Wrote {written} to {addr} (prev = {old_value}, new = {new_value})"
                )]]);
            }
        }
    }

    fn handle_help(command: Option<String>, output: &mut impl Extend<StyledTextOutput>) {
        if let Some(command) = command {
            match command.to_lowercase().as_str() {
                "help" => output.extend([text![
                    bold("help [command]\n"),
                    "Lists available commands, or gets more detailed help for a provided command"
                ]]),
                "addb" => {
                    output.extend([text![
                        bold("addb [breakpoint0] [breakpoint1] .. [breakpointN]\n"),
                        docstr!(
                            /// Creates the listed breakpoints
                            /// The format of allowed breakpoints are as follows:
                            ///     pc:{addr} or {addr} - Adds a breakpoint when the program reaches the given
                            ///                          address
                            ///     r:{addr} - Adds a breakpoint when the program reads from the given address
                            ///     w:{addr} - Adds a breakpoint when the program writes to the given address
                            ///     rw:{addr} - Adds a breakpoint when the program reads from, or writes to
                            ///                 the given address
                            ///     ppu:{mode} - Adds a breakpoint when the PPU enters the given mode
                            ///                  Valid modes: hblank, vblank, oamscan, pixel_transfer
                            ///                  You can also use the respective mode number: (0,1,2,3)
                            ///
                            /// {addr} can be specified as decimal, binary, octal or hexadecimal by using
                            /// the respective prefix (i.e. 0b, 0o or 0x)
                        )
                    ]])
                }
                "delb" => output.extend([text![
                    bold("delb [id0] [id1] .. [idN]\n"),
                    "Delete all breakpoints corresponding to the listed IDs",
                ]]),
                "listb" => output.extend([text![bold("listb\n"), "List all breakpoints"]]),
                "enableb" => output.extend([text![
                    bold("enableb [id0] [id1] .. [idN]\n"),
                    "Enable all breakpoints corresponding to the listed IDs",
                ]]),
                "disableb" => output.extend([text![
                    bold("disableb [id0] [id1] .. [idN]\n"),
                    "Disable all breakpoints corresponding to the listed IDs",
                ]]),
                "frame" => output.extend([text![
                    bold("frame [N=1]\n"),
                    docstr! {
                        /// Steps forward N frames (i.e. to the start of the Nth VBlank), with N defaulting to 1
                        /// if not provided
                    },
                ]]),
                "tick" => output.extend([text![
                    bold("tick [N=1]\n"),
                    docstr! {
                        /// Steps forward N ticks (one T-cycle, which is one tick of the PPU, or 1/4 of a CPU tick),
                        /// with N defaulting to 1 if not provided
                    },
                ]]),
                "dump" => output.extend([text![bold("dump\n"), "Dumps the current CPU state",]]),
                "w" => output.extend([text![
                    bold("w[/b|/o|/x] {addr} {value}\n"),
                    docstr! {
                        /// Writes the given byte to the given address, printing the previous value, and what
                        /// value was actually written.
                        /// The format specifier (/b|/o|/x) can be used to change the display of the values
                        /// to binary, octal or hexadecimal respectively.
                        /// {addr} and {value} can be specified as decimal, binary, octal or hexadecimal by using
                        /// the respective prefix (i.e. 0b, 0o or 0x)
                    },
                ]]),
                "r" => output.extend([text![
                    bold("r[/b|/o|/x] {addr}\n"),
                    docstr! {
                        /// Reads the byte from the given address
                        /// The format specifier (/b|/o|/x) can be used to change the display of the value
                        /// to binary, octal or hexadecimal respectively.
                        /// {addr} can be specified as decimal, binary, octal or hexadecimal by using
                        /// the respective prefix (i.e. 0b, 0o or 0x)
                    },
                ]]),
                unknown => output.extend([text![
                    styled(format!("Unknown command: {unknown}"))
                        .with_color(|palette| palette.red())
                ]]),
            }
        } else {
            output.extend([text![
                "List of commands:\n",
                bold("help [command]"),
                italics(" - lists available commands, or get help for the provided command\n"),
                bold("addb [breakpoint0] [breakpoint1] .. [breakpointN]"),
                italics(" - create breakpoints\n"),
                bold("delb [id0] [id1] .. [idN]"),
                italics(" - delete all listed breakpoints\n"),
                bold("listb"),
                italics(" - list all breakpoints\n"),
                bold("enableb [id0] [id1] .. [idN]"),
                italics(" - enable all listed breakpoints\n"),
                bold("disableb [id0] [id1] .. [idN]"),
                italics(" - disable all listed breakpoints\n"),
                bold("frame"),
                italics(" - step forward one frame (i.e. one VBlank)\n"),
                bold("tick"),
                italics(" - step forward one tick (i.e. one T-cycle)\n"),
                bold("dump"),
                italics(" - dump the current CPU state\n"),
                bold("w[/b|/o|/x] {addr} {value}"),
                italics(" - writes the byte given to the given address\n"),
                bold("r[/b|/o|/x] {addr}"),
                italics(" - reads a byte from the given address\n")
            ]]);
        }
    }
}

pub fn parse_ppu_mode<'src>() -> impl Parser<'src, &'src str, Mode, ErrorTy<'src>> {
    choice((
        choice((just_no_case("vblank"), just("1"))).map(|_| Mode::VBlank),
        choice((just_no_case("hblank"), just("0"))).map(|_| Mode::HBlank),
        choice((just_no_case("pixel_transfer"), just("3"))).map(|_| Mode::PixelTransfer),
        choice((just_no_case("oamscan"), just("2"))).map(|_| Mode::OamScan),
    ))
}

pub fn parse_num<'src, T: Num>() -> impl Parser<'src, &'src str, T, ErrorTy<'src>>
where
    <T as Num>::FromStrRadixErr: std::fmt::Debug + Display,
{
    choice((hexadecimal(), binary(), octal(), decimal()))
}

pub fn hexadecimal<'src, T: Num>() -> impl Parser<'src, &'src str, T, ErrorTy<'src>>
where
    <T as Num>::FromStrRadixErr: std::fmt::Debug + Display,
{
    just_no_case("0x")
        .ignore_then(
            digits(16)
                .labelled("hexadecimal digit")
                .to_slice()
                .labelled("hexadecimal digit segment")
                .separated_by(just('_').repeated().at_least(1))
                .at_least(1)
                .collect::<String>()
                .try_map(|result, span| {
                    T::from_str_radix(&result, 16).map_err(|err| {
                        Rich::custom(span, format!("Invalid number: {result}, {err}"))
                    })
                }),
        )
        .labelled("hexadecimal number")
}

pub fn decimal<'src, T: Num>() -> impl Parser<'src, &'src str, T, ErrorTy<'src>>
where
    <T as Num>::FromStrRadixErr: std::fmt::Debug + Display,
{
    digits(10)
        .labelled("decimal digit")
        .to_slice()
        .labelled("decimal digit segment")
        .separated_by(just('_').repeated().at_least(1))
        .at_least(1)
        .collect::<String>()
        .try_map(|result, span| {
            T::from_str_radix(&result, 10)
                .map_err(|err| Rich::custom(span, format!("Invalid number: {result}, {err}")))
        })
        .labelled("decimal number")
}

pub fn binary<'src, T: Num>() -> impl Parser<'src, &'src str, T, ErrorTy<'src>>
where
    <T as Num>::FromStrRadixErr: std::fmt::Debug + Display,
{
    just_no_case("0b")
        .ignore_then(
            digits(2)
                .labelled("binary digit")
                .to_slice()
                .labelled("binary digit segment")
                .separated_by(just('_').repeated().at_least(1).labelled("separator"))
                .at_least(1)
                .collect::<String>()
                .try_map(|result, span| {
                    T::from_str_radix(&result, 2).map_err(|err| {
                        Rich::custom(span, format!("Invalid number: {result}, {err}"))
                    })
                }),
        )
        .labelled("binary number")
}

pub fn octal<'src, T: Num>() -> impl Parser<'src, &'src str, T, ErrorTy<'src>>
where
    <T as Num>::FromStrRadixErr: std::fmt::Debug + Display,
{
    just_no_case("0o")
        .ignore_then(
            digits(8)
                .labelled("octal digit")
                .to_slice()
                .labelled("octal digit segment")
                .separated_by(just('_').repeated().at_least(1))
                .at_least(1)
                .collect::<String>()
                .try_map(|result, span| {
                    T::from_str_radix(&result, 8).map_err(|err| {
                        Rich::custom(span, format!("Invalid number: {result}, {err}"))
                    })
                }),
        )
        .labelled("octal number")
}

pub fn styled(text: impl AsRef<str>) -> StyledText {
    StyledText::from(text)
}

pub fn bold(text: impl AsRef<str>) -> StyledText {
    StyledText::from(text).with_weight(TextWeight::Bold)
}

pub fn italics(text: impl AsRef<str>) -> StyledText {
    StyledText::from(text).with_style(TextStyle::Italics)
}

pub fn format_radix_u8(radix: Radix, value: u8) -> String {
    match radix {
        Radix::Decimal => {
            format!("{value}")
        }
        Radix::Binary => {
            format!("0b{value:08b}")
        }
        Radix::Octal => {
            format!("0o{value:03o}")
        }
        Radix::Hexadecimal => {
            format!("0x{value:02X}")
        }
    }
}

pub fn format_radix_u16(radix: Radix, value: u16) -> String {
    match radix {
        Radix::Decimal => {
            format!("{value}")
        }
        Radix::Binary => {
            format!("0b{value:016b}")
        }
        Radix::Octal => {
            format!("0o{value:06o}")
        }
        Radix::Hexadecimal => {
            format!("0x{value:04X}")
        }
    }
}
