#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelForm {
    Fn,
    Def,
    Let,
    Do,
    If,
    Type,
    Struct,
    New,
    Field,
    Quote,
    Macro,
    Rust,
    RustTrait,
    Use,
    Export,
}

impl KernelForm {
    pub const ALL: [KernelForm; 15] = [
        KernelForm::Fn,
        KernelForm::Def,
        KernelForm::Let,
        KernelForm::Do,
        KernelForm::If,
        KernelForm::Type,
        KernelForm::Struct,
        KernelForm::New,
        KernelForm::Field,
        KernelForm::Quote,
        KernelForm::Macro,
        KernelForm::Rust,
        KernelForm::RustTrait,
        KernelForm::Use,
        KernelForm::Export,
    ];

    pub fn name(self) -> &'static str {
        match self {
            KernelForm::Fn => "fn",
            KernelForm::Def => "def",
            KernelForm::Let => "let",
            KernelForm::Do => "do",
            KernelForm::If => "if",
            KernelForm::Type => "type",
            KernelForm::Struct => "struct",
            KernelForm::New => "new",
            KernelForm::Field => "field",
            KernelForm::Quote => "quote",
            KernelForm::Macro => "macro",
            KernelForm::Rust => "rust",
            KernelForm::RustTrait => "rust-trait",
            KernelForm::Use => "use",
            KernelForm::Export => "export",
        }
    }

    pub fn shape(self) -> &'static str {
        match self {
            KernelForm::Fn => "(fn (param ...) body ...)",
            KernelForm::Def => "(def name value)",
            KernelForm::Let => "(let name value)",
            KernelForm::Do => "(do form ...)",
            KernelForm::If => "(if test then else)",
            KernelForm::Type => "(type name type)",
            KernelForm::Struct => "(struct name (field type) ...)",
            KernelForm::New => "(new name (field value) ...)",
            KernelForm::Field => "(field value name)",
            KernelForm::Quote => "(quote form)",
            KernelForm::Macro => "(def name (macro (form) body ...))",
            KernelForm::Rust => "(rust \"template\" type twin)",
            KernelForm::RustTrait => "(rust-trait name \"Path\" type ...)",
            KernelForm::Use => "(use module name ...)",
            KernelForm::Export => "(export name ...)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Bool,
    Char,
    Str,
    String,
    Unit,
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
    F32,
    F64,
}

impl PrimitiveType {
    pub const ALL: [PrimitiveType; 19] = [
        PrimitiveType::Bool,
        PrimitiveType::Char,
        PrimitiveType::Str,
        PrimitiveType::String,
        PrimitiveType::Unit,
        PrimitiveType::I8,
        PrimitiveType::I16,
        PrimitiveType::I32,
        PrimitiveType::I64,
        PrimitiveType::I128,
        PrimitiveType::Isize,
        PrimitiveType::U8,
        PrimitiveType::U16,
        PrimitiveType::U32,
        PrimitiveType::U64,
        PrimitiveType::U128,
        PrimitiveType::Usize,
        PrimitiveType::F32,
        PrimitiveType::F64,
    ];

    pub fn is_integer(self) -> bool {
        matches!(
            self,
            PrimitiveType::I8
                | PrimitiveType::I16
                | PrimitiveType::I32
                | PrimitiveType::I64
                | PrimitiveType::I128
                | PrimitiveType::Isize
                | PrimitiveType::U8
                | PrimitiveType::U16
                | PrimitiveType::U32
                | PrimitiveType::U64
                | PrimitiveType::U128
                | PrimitiveType::Usize
        )
    }

    pub fn is_float(self) -> bool {
        matches!(self, PrimitiveType::F32 | PrimitiveType::F64)
    }

    pub fn name(self) -> &'static str {
        match self {
            PrimitiveType::Bool => "bool",
            PrimitiveType::Char => "char",
            PrimitiveType::Str => "str",
            PrimitiveType::String => "string",
            PrimitiveType::Unit => "unit",
            PrimitiveType::I8 => "i8",
            PrimitiveType::I16 => "i16",
            PrimitiveType::I32 => "i32",
            PrimitiveType::I64 => "i64",
            PrimitiveType::I128 => "i128",
            PrimitiveType::Isize => "isize",
            PrimitiveType::U8 => "u8",
            PrimitiveType::U16 => "u16",
            PrimitiveType::U32 => "u32",
            PrimitiveType::U64 => "u64",
            PrimitiveType::U128 => "u128",
            PrimitiveType::Usize => "usize",
            PrimitiveType::F32 => "f32",
            PrimitiveType::F64 => "f64",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelType {
    Function,
    Ref,
    Mut,
    Where,
    Hole,
    Primitive(PrimitiveType),
}

impl KernelType {
    pub fn all() -> impl Iterator<Item = KernelType> {
        [
            KernelType::Function,
            KernelType::Ref,
            KernelType::Mut,
            KernelType::Where,
            KernelType::Hole,
        ]
        .into_iter()
        .chain(PrimitiveType::ALL.into_iter().map(KernelType::Primitive))
    }

    pub fn shape(self) -> &'static str {
        match self {
            KernelType::Function => "(fn (argument ...) result)",
            KernelType::Ref => "(ref type)",
            KernelType::Mut => "(mut type)",
            KernelType::Where => "(where (variable (variable trait ...) ...) type)",
            KernelType::Hole | KernelType::Primitive(_) => self.name(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            KernelType::Function => "fn",
            KernelType::Ref => "ref",
            KernelType::Mut => "mut",
            KernelType::Where => "where",
            KernelType::Hole => "_",
            KernelType::Primitive(primitive) => primitive.name(),
        }
    }
}

pub struct StructDerives;

impl StructDerives {
    pub const TRAITS: [&'static str; 1] = ["Clone"];

    pub fn attribute() -> String {
        format!("#[derive({})]", Self::TRAITS.join(", "))
    }
}
