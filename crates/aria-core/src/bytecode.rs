use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The V3.1 compiled-program container. The runtime never executes source
/// text: it validates this target-independent binary representation first.
pub const ARIAC_FORMAT_VERSION: u16 = 4;
pub const ARIAC_MAGIC: [u8; 8] = *b"ARIAC4\0\0";
pub const ARIAC_VM_ABI_VERSION: u16 = 1;

const CHECKSUM_LENGTH: usize = 32;
const FIXED_HEADER_LENGTH: usize = 34;
const MAX_BODY_LENGTH: usize = 256 * 1024 * 1024;
const MAX_TABLE_ENTRIES: usize = 1_000_000;
const MAX_STRING_LENGTH: usize = 16 * 1024 * 1024;
const FLAG_SOURCE_MAP: u16 = 0b0000_0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageVersion {
    pub major: u16,
    pub minor: u16,
}

impl LanguageVersion {
    /// The alpha 3.0 command language. This value exists only while the
    /// migration tool converts existing V3 alpha projects.
    pub const V3_0: Self = Self { major: 3, minor: 0 };
    /// The structured, typed author language introduced for the 1.0 line.
    pub const V3_1: Self = Self { major: 3, minor: 1 };
    /// Compatibility alias for old embedding callers. New code should use an
    /// explicit language version; release packages must use [`Self::V3_1`].
    pub const V3: Self = Self::V3_0;

    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::V3_0 | Self::V3_1)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Constant {
    String(String),
    Integer(i64),
    Float(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteOp {
    Nop,
    Text,
    WaitAdvance,
    TextClear,
    Delay,
    Jump,
    JumpIfFalse,
    Call,
    Return,
    SetInt,
    AddInt,
    SetString,
    Background,
    SpriteImage,
    SpriteRect,
    SpriteText,
    SpriteRemove,
    SpriteVisibility,
    SpriteMove,
    PresentChoice,
    PlayAudio,
    StopAudio,
    SetVolume,
    BeginTransition,
    Save,
    Load,
    End,
    /// Compatibility-only opcode for the alpha 3.0 source bridge. Aria 3.1
    /// never emits this opcode and release validation rejects it.
    Host,
}

impl ByteOp {
    const fn code(self) -> u8 {
        match self {
            Self::Nop => 0,
            Self::Text => 1,
            Self::WaitAdvance => 2,
            Self::TextClear => 3,
            Self::Delay => 4,
            Self::Jump => 5,
            Self::JumpIfFalse => 6,
            Self::Call => 7,
            Self::Return => 8,
            Self::SetInt => 9,
            Self::AddInt => 10,
            Self::SetString => 11,
            Self::Background => 12,
            Self::SpriteImage => 13,
            Self::SpriteRect => 14,
            Self::SpriteText => 15,
            Self::SpriteRemove => 16,
            Self::SpriteVisibility => 17,
            Self::SpriteMove => 18,
            Self::PresentChoice => 19,
            Self::PlayAudio => 20,
            Self::StopAudio => 21,
            Self::SetVolume => 22,
            Self::BeginTransition => 23,
            Self::Save => 24,
            Self::Load => 25,
            Self::End => 26,
            Self::Host => 27,
        }
    }

    fn from_code(code: u8) -> Result<Self, AriacError> {
        match code {
            0 => Ok(Self::Nop),
            1 => Ok(Self::Text),
            2 => Ok(Self::WaitAdvance),
            3 => Ok(Self::TextClear),
            4 => Ok(Self::Delay),
            5 => Ok(Self::Jump),
            6 => Ok(Self::JumpIfFalse),
            7 => Ok(Self::Call),
            8 => Ok(Self::Return),
            9 => Ok(Self::SetInt),
            10 => Ok(Self::AddInt),
            11 => Ok(Self::SetString),
            12 => Ok(Self::Background),
            13 => Ok(Self::SpriteImage),
            14 => Ok(Self::SpriteRect),
            15 => Ok(Self::SpriteText),
            16 => Ok(Self::SpriteRemove),
            17 => Ok(Self::SpriteVisibility),
            18 => Ok(Self::SpriteMove),
            19 => Ok(Self::PresentChoice),
            20 => Ok(Self::PlayAudio),
            21 => Ok(Self::StopAudio),
            22 => Ok(Self::SetVolume),
            23 => Ok(Self::BeginTransition),
            24 => Ok(Self::Save),
            25 => Ok(Self::Load),
            26 => Ok(Self::End),
            27 => Ok(Self::Host),
            value => Err(AriacError::InvalidOpcode(value)),
        }
    }

    fn validates_arity(self, operand_count: usize) -> bool {
        match self {
            Self::Nop | Self::TextClear | Self::Return | Self::End => operand_count == 0,
            Self::Text
            | Self::SetInt
            | Self::SetString
            | Self::Background
            | Self::SpriteVisibility
            | Self::BeginTransition
            | Self::Host => operand_count == 2,
            Self::WaitAdvance
            | Self::Delay
            | Self::Jump
            | Self::Call
            | Self::SpriteRemove
            | Self::Save
            | Self::Load => operand_count == 1,
            Self::JumpIfFalse => operand_count == 4,
            Self::AddInt | Self::SpriteMove | Self::StopAudio | Self::SetVolume => {
                operand_count == 3
            }
            Self::SpriteImage | Self::SpriteText | Self::PlayAudio => operand_count == 6,
            Self::SpriteRect => operand_count == 7,
            Self::PresentChoice => operand_count >= 2 && operand_count.is_multiple_of(2),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Operand {
    Constant(u32),
    Integer(i64),
    Float(f32),
    Boolean(bool),
    IntRegister(String),
    StringRegister(String),
    Address(u32),
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedInstruction {
    pub op: ByteOp,
    pub operands: Vec<Operand>,
}

impl EncodedInstruction {
    #[must_use]
    pub fn new(op: ByteOp, operands: impl Into<Vec<Operand>>) -> Self {
        Self {
            op,
            operands: operands.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub source: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledProgram {
    pub format_version: u16,
    pub language_version: LanguageVersion,
    pub game_id: String,
    pub constants: Vec<Constant>,
    pub instructions: Vec<EncodedInstruction>,
    pub source_map: Vec<SourceLocation>,
}

impl CompiledProgram {
    #[must_use]
    pub fn empty(game_id: impl Into<String>) -> Self {
        Self {
            format_version: ARIAC_FORMAT_VERSION,
            language_version: LanguageVersion::V3_1,
            game_id: game_id.into(),
            constants: Vec::new(),
            instructions: vec![EncodedInstruction::new(ByteOp::End, Vec::new())],
            source_map: vec![SourceLocation {
                source: "<generated>".to_owned(),
                line: 1,
                column: 1,
            }],
        }
    }

    pub fn validate(&self) -> Result<(), AriacError> {
        if self.format_version != ARIAC_FORMAT_VERSION {
            return Err(AriacError::UnsupportedFormat(self.format_version));
        }
        if !self.language_version.is_supported() {
            return Err(AriacError::UnsupportedLanguage {
                major: self.language_version.major,
                minor: self.language_version.minor,
            });
        }
        validate_string(&self.game_id, "game ID")?;
        if self.game_id.trim().is_empty() {
            return Err(AriacError::InvalidProgram("game ID is empty".to_owned()));
        }
        if self.constants.len() > MAX_TABLE_ENTRIES
            || self.instructions.len() > MAX_TABLE_ENTRIES
            || self.source_map.len() > MAX_TABLE_ENTRIES
        {
            return Err(AriacError::TooManyTableEntries);
        }
        if self.instructions.is_empty() {
            return Err(AriacError::InvalidProgram(
                "instruction table is empty".to_owned(),
            ));
        }
        if self.instructions.len() != self.source_map.len() {
            return Err(AriacError::InvalidProgram(
                "source map length differs from instruction table".to_owned(),
            ));
        }

        for constant in &self.constants {
            match constant {
                Constant::String(value) => validate_string(value, "constant string")?,
                Constant::Integer(_) => {}
                Constant::Float(value) if value.is_finite() => {}
                Constant::Float(_) => {
                    return Err(AriacError::InvalidProgram(
                        "constant float must be finite".to_owned(),
                    ));
                }
            }
        }
        for location in &self.source_map {
            validate_string(&location.source, "source-map path")?;
            if location.line == 0 || location.column == 0 {
                return Err(AriacError::InvalidProgram(
                    "source-map line and column start at one".to_owned(),
                ));
            }
        }

        let instruction_count = self.instructions.len();
        for (index, instruction) in self.instructions.iter().enumerate() {
            if !instruction.op.validates_arity(instruction.operands.len()) {
                return Err(AriacError::InvalidProgram(format!(
                    "instruction {index} has an invalid operand count for {:?}",
                    instruction.op
                )));
            }
            validate_instruction_schema(index, instruction)?;
            for operand in &instruction.operands {
                match operand {
                    Operand::Constant(constant)
                        if usize::try_from(*constant)
                            .ok()
                            .is_none_or(|constant| constant >= self.constants.len()) =>
                    {
                        return Err(AriacError::InvalidProgram(format!(
                            "instruction {index} references missing constant {constant}"
                        )));
                    }
                    Operand::Address(address)
                        if usize::try_from(*address)
                            .ok()
                            .is_none_or(|address| address >= instruction_count) =>
                    {
                        return Err(AriacError::InvalidProgram(format!(
                            "instruction {index} references missing address {address}"
                        )));
                    }
                    Operand::Float(value) if !value.is_finite() => {
                        return Err(AriacError::InvalidProgram(format!(
                            "instruction {index} contains a non-finite float"
                        )));
                    }
                    Operand::IntRegister(name) | Operand::StringRegister(name) => {
                        validate_register_name(name, index)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Encodes the program into a deterministic, little-endian binary format.
    /// The payload deliberately contains no JSON, host pointers, platform
    /// paths, or runtime-specific types.
    pub fn encode(&self) -> Result<Vec<u8>, AriacError> {
        self.validate()?;

        let mut body = Vec::new();
        for constant in &self.constants {
            encode_constant(&mut body, constant)?;
        }
        for instruction in &self.instructions {
            body.push(instruction.op.code());
            push_u16(
                &mut body,
                u16::try_from(instruction.operands.len())
                    .map_err(|_| AriacError::TooManyOperands)?,
            );
            for operand in &instruction.operands {
                encode_operand(&mut body, operand)?;
            }
        }
        for location in &self.source_map {
            push_string(&mut body, &location.source)?;
            push_u32(&mut body, location.line);
            push_u32(&mut body, location.column);
        }
        if body.len() > MAX_BODY_LENGTH {
            return Err(AriacError::PayloadTooLarge(body.len()));
        }

        let game_id = self.game_id.as_bytes();
        let mut checked = Vec::with_capacity(
            FIXED_HEADER_LENGTH
                .saturating_add(game_id.len())
                .saturating_add(body.len()),
        );
        push_u16(&mut checked, self.format_version);
        push_u16(&mut checked, self.language_version.major);
        push_u16(&mut checked, self.language_version.minor);
        push_u16(&mut checked, ARIAC_VM_ABI_VERSION);
        push_u16(&mut checked, FLAG_SOURCE_MAP);
        push_u32(
            &mut checked,
            u32::try_from(game_id.len())
                .map_err(|_| AriacError::InvalidProgram("game ID is too long".to_owned()))?,
        );
        push_u32(
            &mut checked,
            u32::try_from(self.constants.len()).map_err(|_| AriacError::TooManyTableEntries)?,
        );
        push_u32(
            &mut checked,
            u32::try_from(self.instructions.len()).map_err(|_| AriacError::TooManyTableEntries)?,
        );
        push_u32(
            &mut checked,
            u32::try_from(self.source_map.len()).map_err(|_| AriacError::TooManyTableEntries)?,
        );
        push_u64(
            &mut checked,
            u64::try_from(body.len()).expect("body length fits in u64"),
        );
        checked.extend_from_slice(game_id);
        checked.extend_from_slice(&body);

        let checksum = blake3::hash(&checked);
        let mut encoded = Vec::with_capacity(ARIAC_MAGIC.len() + checked.len() + CHECKSUM_LENGTH);
        encoded.extend_from_slice(&ARIAC_MAGIC);
        encoded.extend_from_slice(&checked);
        encoded.extend_from_slice(checksum.as_bytes());
        Ok(encoded)
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, AriacError> {
        let minimum = ARIAC_MAGIC.len() + FIXED_HEADER_LENGTH + CHECKSUM_LENGTH;
        if encoded.len() < minimum {
            return Err(AriacError::Truncated);
        }
        if encoded[..ARIAC_MAGIC.len()] != ARIAC_MAGIC {
            return Err(AriacError::InvalidMagic);
        }

        let checked_end = encoded.len() - CHECKSUM_LENGTH;
        let checked = &encoded[ARIAC_MAGIC.len()..checked_end];
        let expected = blake3::hash(checked);
        if expected.as_bytes() != &encoded[checked_end..] {
            return Err(AriacError::ChecksumMismatch);
        }

        let mut header = Reader::new(checked);
        let format_version = header.u16()?;
        let language_version = LanguageVersion {
            major: header.u16()?,
            minor: header.u16()?,
        };
        let abi_version = header.u16()?;
        if abi_version != ARIAC_VM_ABI_VERSION {
            return Err(AriacError::UnsupportedVmAbi(abi_version));
        }
        let flags = header.u16()?;
        if flags != FLAG_SOURCE_MAP {
            return Err(AriacError::InvalidFlags(flags));
        }
        let game_id_length = usize::try_from(header.u32()?).map_err(|_| AriacError::Truncated)?;
        let constant_count = count(header.u32()?)?;
        let instruction_count = count(header.u32()?)?;
        let source_count = count(header.u32()?)?;
        let body_length =
            usize::try_from(header.u64()?).map_err(|_| AriacError::PayloadTooLarge(usize::MAX))?;
        if game_id_length > MAX_STRING_LENGTH {
            return Err(AriacError::StringTooLong(game_id_length));
        }
        if body_length > MAX_BODY_LENGTH {
            return Err(AriacError::PayloadTooLarge(body_length));
        }
        if source_count != instruction_count {
            return Err(AriacError::HeaderMismatch);
        }
        let game_id = header.string_with_length(game_id_length)?;
        let body = header.bytes(body_length)?;
        if !header.is_finished() {
            return Err(AriacError::HeaderMismatch);
        }

        let mut reader = Reader::new(body);
        let mut constants = Vec::with_capacity(constant_count);
        for _ in 0..constant_count {
            constants.push(decode_constant(&mut reader)?);
        }
        let mut instructions = Vec::with_capacity(instruction_count);
        for _ in 0..instruction_count {
            let op = ByteOp::from_code(reader.u8()?)?;
            let operand_count = usize::from(reader.u16()?);
            if operand_count > u16::MAX as usize {
                return Err(AriacError::TooManyOperands);
            }
            let mut operands = Vec::with_capacity(operand_count);
            for _ in 0..operand_count {
                operands.push(decode_operand(&mut reader)?);
            }
            instructions.push(EncodedInstruction { op, operands });
        }
        let mut source_map = Vec::with_capacity(source_count);
        for _ in 0..source_count {
            source_map.push(SourceLocation {
                source: reader.string()?,
                line: reader.u32()?,
                column: reader.u32()?,
            });
        }
        if !reader.is_finished() {
            return Err(AriacError::HeaderMismatch);
        }

        let program = Self {
            format_version,
            language_version,
            game_id,
            constants,
            instructions,
            source_map,
        };
        program.validate()?;
        Ok(program)
    }
}

fn validate_string(value: &str, label: &str) -> Result<(), AriacError> {
    if value.len() > MAX_STRING_LENGTH {
        return Err(AriacError::StringTooLong(value.len()));
    }
    if value.as_bytes().contains(&0) {
        return Err(AriacError::InvalidProgram(format!(
            "{label} contains a NUL byte"
        )));
    }
    Ok(())
}

fn validate_register_name(name: &str, instruction: usize) -> Result<(), AriacError> {
    validate_string(name, "register name")?;
    if name.is_empty() {
        return Err(AriacError::InvalidProgram(format!(
            "instruction {instruction} has an empty register name"
        )));
    }
    Ok(())
}

fn validate_instruction_schema(
    index: usize,
    instruction: &EncodedInstruction,
) -> Result<(), AriacError> {
    let operands = &instruction.operands;
    let require_address = |operand: &Operand, position: usize| {
        matches!(operand, Operand::Address(_))
            .then_some(())
            .ok_or_else(|| {
                AriacError::InvalidProgram(format!(
                    "instruction {index} {:?} operand {position} must be an address",
                    instruction.op
                ))
            })
    };
    let require_int_register = |operand: &Operand| {
        matches!(operand, Operand::IntRegister(_))
            .then_some(())
            .ok_or_else(|| {
                AriacError::InvalidProgram(format!(
                    "instruction {index} {:?} requires an integer register target",
                    instruction.op
                ))
            })
    };
    let require_string_register = |operand: &Operand| {
        matches!(operand, Operand::StringRegister(_))
            .then_some(())
            .ok_or_else(|| {
                AriacError::InvalidProgram(format!(
                    "instruction {index} {:?} requires a string register target",
                    instruction.op
                ))
            })
    };

    match instruction.op {
        ByteOp::Jump | ByteOp::Call => require_address(&operands[0], 0),
        ByteOp::JumpIfFalse => require_address(&operands[3], 3),
        ByteOp::SetInt | ByteOp::AddInt => require_int_register(&operands[0]),
        ByteOp::SetString => require_string_register(&operands[0]),
        ByteOp::PresentChoice => {
            for (position, operand) in operands.iter().enumerate().skip(1).step_by(2) {
                require_address(operand, position)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn encode_constant(output: &mut Vec<u8>, constant: &Constant) -> Result<(), AriacError> {
    match constant {
        Constant::String(value) => {
            output.push(0);
            push_string(output, value)?;
        }
        Constant::Integer(value) => {
            output.push(1);
            push_i64(output, *value);
        }
        Constant::Float(value) => {
            output.push(2);
            push_f64(output, *value);
        }
    }
    Ok(())
}

fn decode_constant(reader: &mut Reader<'_>) -> Result<Constant, AriacError> {
    match reader.u8()? {
        0 => Ok(Constant::String(reader.string()?)),
        1 => Ok(Constant::Integer(reader.i64()?)),
        2 => Ok(Constant::Float(reader.f64()?)),
        tag => Err(AriacError::InvalidConstantTag(tag)),
    }
}

fn encode_operand(output: &mut Vec<u8>, operand: &Operand) -> Result<(), AriacError> {
    match operand {
        Operand::Constant(value) => {
            output.push(0);
            push_u32(output, *value);
        }
        Operand::Integer(value) => {
            output.push(1);
            push_i64(output, *value);
        }
        Operand::Float(value) => {
            output.push(2);
            push_f32(output, *value);
        }
        Operand::Boolean(value) => {
            output.push(3);
            output.push(u8::from(*value));
        }
        Operand::IntRegister(value) => {
            output.push(4);
            push_string(output, value)?;
        }
        Operand::StringRegister(value) => {
            output.push(5);
            push_string(output, value)?;
        }
        Operand::Address(value) => {
            output.push(6);
            push_u32(output, *value);
        }
        Operand::None => output.push(7),
    }
    Ok(())
}

fn decode_operand(reader: &mut Reader<'_>) -> Result<Operand, AriacError> {
    match reader.u8()? {
        0 => Ok(Operand::Constant(reader.u32()?)),
        1 => Ok(Operand::Integer(reader.i64()?)),
        2 => Ok(Operand::Float(reader.f32()?)),
        3 => match reader.u8()? {
            0 => Ok(Operand::Boolean(false)),
            1 => Ok(Operand::Boolean(true)),
            value => Err(AriacError::InvalidBoolean(value)),
        },
        4 => Ok(Operand::IntRegister(reader.string()?)),
        5 => Ok(Operand::StringRegister(reader.string()?)),
        6 => Ok(Operand::Address(reader.u32()?)),
        7 => Ok(Operand::None),
        tag => Err(AriacError::InvalidOperandTag(tag)),
    }
}

fn count(value: u32) -> Result<usize, AriacError> {
    let value = usize::try_from(value).map_err(|_| AriacError::TooManyTableEntries)?;
    if value > MAX_TABLE_ENTRIES {
        Err(AriacError::TooManyTableEntries)
    } else {
        Ok(value)
    }
}

fn push_string(output: &mut Vec<u8>, value: &str) -> Result<(), AriacError> {
    validate_string(value, "encoded string")?;
    push_u32(
        output,
        u32::try_from(value.len()).map_err(|_| AriacError::StringTooLong(value.len()))?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(output: &mut Vec<u8>, value: f32) {
    output.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn push_f64(output: &mut Vec<u8>, value: f64) {
    output.extend_from_slice(&value.to_bits().to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    const fn is_finished(&self) -> bool {
        self.cursor == self.bytes.len()
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], AriacError> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or(AriacError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .ok_or(AriacError::Truncated)?;
        self.cursor = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, AriacError> {
        Ok(*self.bytes(1)?.first().ok_or(AriacError::Truncated)?)
    }

    fn u16(&mut self) -> Result<u16, AriacError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, AriacError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, AriacError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn i64(&mut self) -> Result<i64, AriacError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, AriacError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.array()?)))
    }

    fn f64(&mut self) -> Result<f64, AriacError> {
        Ok(f64::from_bits(u64::from_le_bytes(self.array()?)))
    }

    fn string(&mut self) -> Result<String, AriacError> {
        let length = usize::try_from(self.u32()?).map_err(|_| AriacError::Truncated)?;
        self.string_with_length(length)
    }

    fn string_with_length(&mut self, length: usize) -> Result<String, AriacError> {
        if length > MAX_STRING_LENGTH {
            return Err(AriacError::StringTooLong(length));
        }
        let bytes = self.bytes(length)?;
        let value = std::str::from_utf8(bytes)
            .map_err(AriacError::InvalidUtf8)?
            .to_owned();
        validate_string(&value, "decoded string")?;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], AriacError> {
        self.bytes(N)?.try_into().map_err(|_| AriacError::Truncated)
    }
}

#[derive(Debug, Error)]
pub enum AriacError {
    #[error("invalid .ariac magic")]
    InvalidMagic,
    #[error("truncated or length-mismatched .ariac file")]
    Truncated,
    #[error(".ariac checksum mismatch")]
    ChecksumMismatch,
    #[error("unsupported .ariac format version {0}")]
    UnsupportedFormat(u16),
    #[error("unsupported Aria language version {major}.{minor}")]
    UnsupportedLanguage { major: u16, minor: u16 },
    #[error("unsupported Aria VM ABI version {0}")]
    UnsupportedVmAbi(u16),
    #[error("invalid .ariac flags {0:#x}")]
    InvalidFlags(u16),
    #[error(".ariac header and body metadata differ")]
    HeaderMismatch,
    #[error("invalid compiled program: {0}")]
    InvalidProgram(String),
    #[error(".ariac payload is too large: {0} bytes")]
    PayloadTooLarge(usize),
    #[error(".ariac contains too many table entries")]
    TooManyTableEntries,
    #[error(".ariac instruction has too many operands")]
    TooManyOperands,
    #[error(".ariac string is too long: {0} bytes")]
    StringTooLong(usize),
    #[error("invalid .ariac opcode {0}")]
    InvalidOpcode(u8),
    #[error("invalid .ariac constant tag {0}")]
    InvalidConstantTag(u8),
    #[error("invalid .ariac operand tag {0}")]
    InvalidOperandTag(u8),
    #[error("invalid .ariac boolean value {0}")]
    InvalidBoolean(u8),
    #[error("invalid UTF-8 in .ariac: {0}")]
    InvalidUtf8(std::str::Utf8Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ariac4_round_trips_and_is_binary_deterministic() {
        let program = CompiledProgram {
            format_version: ARIAC_FORMAT_VERSION,
            language_version: LanguageVersion::V3_1,
            game_id: "jp.example.game".to_owned(),
            constants: vec![Constant::String("海へ行こう。".to_owned())],
            instructions: vec![
                EncodedInstruction::new(ByteOp::Text, vec![Operand::None, Operand::Constant(0)]),
                EncodedInstruction::new(ByteOp::WaitAdvance, vec![Operand::Boolean(false)]),
                EncodedInstruction::new(ByteOp::End, Vec::new()),
            ],
            source_map: vec![
                SourceLocation {
                    source: "scripts/main.aria".to_owned(),
                    line: 3,
                    column: 3,
                },
                SourceLocation {
                    source: "scripts/main.aria".to_owned(),
                    line: 3,
                    column: 3,
                },
                SourceLocation {
                    source: "scripts/main.aria".to_owned(),
                    line: 4,
                    column: 1,
                },
            ],
        };
        let encoded = program.encode().unwrap();
        assert_eq!(&encoded[..ARIAC_MAGIC.len()], &ARIAC_MAGIC);
        assert!(
            !encoded
                .windows(b"\"instructions\"".len())
                .any(|window| window == b"\"instructions\"")
        );
        assert_eq!(program.encode().unwrap(), encoded);
        assert_eq!(CompiledProgram::decode(&encoded).unwrap(), program);
    }

    #[test]
    fn ariac4_rejects_corruption_and_unknown_opcode() {
        let program = CompiledProgram::empty("jp.example.game");
        let encoded = program.encode().unwrap();
        let mut corrupt = encoded.clone();
        let index = corrupt.len() / 2;
        corrupt[index] ^= 0x40;
        assert!(matches!(
            CompiledProgram::decode(&corrupt),
            Err(AriacError::ChecksumMismatch)
        ));

        let mut invalid_opcode = encoded;
        let body_start = ARIAC_MAGIC.len() + FIXED_HEADER_LENGTH + program.game_id.len();
        invalid_opcode[body_start] = 0xff;
        let checksum_start = invalid_opcode.len() - CHECKSUM_LENGTH;
        let checksum = blake3::hash(&invalid_opcode[ARIAC_MAGIC.len()..checksum_start]);
        invalid_opcode[checksum_start..].copy_from_slice(checksum.as_bytes());
        assert!(matches!(
            CompiledProgram::decode(&invalid_opcode),
            Err(AriacError::InvalidOpcode(0xff))
        ));
    }

    #[test]
    fn validator_rejects_invalid_control_flow_and_nonfinite_values() {
        let mut program = CompiledProgram::empty("jp.example.game");
        program.instructions[0] = EncodedInstruction::new(ByteOp::Jump, vec![Operand::Address(9)]);
        assert!(matches!(
            program.validate(),
            Err(AriacError::InvalidProgram(_))
        ));

        let mut program = CompiledProgram::empty("jp.example.game");
        program.constants.push(Constant::Float(f64::NAN));
        assert!(matches!(
            program.validate(),
            Err(AriacError::InvalidProgram(_))
        ));
    }
}
