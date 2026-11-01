use super::ClosureKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelForm {
    Expression(ExpressionForm),
    Definition(DefinitionForm),
    Let,
    Macro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionForm {
    Fn,
    Recur,
    Try,
    Leaf(LeafForm),
    Borrow(ReferenceKind),
    Deref,
    Control(ControlForm),
    Struct(StructForm),
    Match,
    Assoc,
    MapAt,
    SetAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeafForm {
    Quote,
    Rust,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlForm {
    Do,
    If,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructForm {
    New,
    Field,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionForm {
    Def,
    Type,
    Struct,
    Enum,
    Derive,
    RustTrait,
    RustType,
    Trait,
    Impl,
    Use,
    Export,
}

struct KernelFormRow {
    form: KernelForm,
    name: &'static str,
    shape: &'static str,
}

impl KernelForm {
    const ROWS: [KernelFormRow; 29] = [
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Fn),
            name: "fn",
            shape: "(fn (param ...) body ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Def),
            name: "def",
            shape: "(def name value)",
        },
        KernelFormRow {
            form: KernelForm::Let,
            name: "let",
            shape: "(let name value)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Control(ControlForm::Do)),
            name: "do",
            shape: "(do form ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Control(ControlForm::If)),
            name: "if",
            shape: "(if test then else)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Type),
            name: "type",
            shape: "(type name type)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Struct),
            name: "struct",
            shape: "(struct name (field type) ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Enum),
            name: "enum",
            shape: "(enum name variant ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Derive),
            name: "derive",
            shape: "(derive type trait ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Match),
            name: "match",
            shape: "(match value (pattern body) ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Assoc),
            name: "assoc-at",
            shape: "(assoc-at value (key ...) item)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::MapAt),
            name: "map-at",
            shape: "(map-at operation value argument ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::SetAt),
            name: "set-at",
            shape: "(set-at value (key ...) item)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Struct(StructForm::New)),
            name: "new",
            shape: "(new name (field value) ...) or (new _ base (field value) ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Struct(StructForm::Field)),
            name: "field",
            shape: "(field value name)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Borrow(ReferenceKind::Shared)),
            name: "ref",
            shape: "(ref value)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Borrow(ReferenceKind::Mutable)),
            name: "mut",
            shape: "(mut place)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Deref),
            name: "deref",
            shape: "(deref value)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Leaf(LeafForm::Quote)),
            name: "quote",
            shape: "(quote form)",
        },
        KernelFormRow {
            form: KernelForm::Macro,
            name: "macro",
            shape: "(def name (macro (form) body ...))",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Leaf(LeafForm::Rust)),
            name: "rust",
            shape: "(rust \"template\" type)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::RustTrait),
            name: "rust-trait",
            shape: "(rust-trait name \"Path\" (method name type)? (help \"text\")? type ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::RustType),
            name: "rust-type",
            shape: "(rust-type name \"template\" (type \"template\") ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Recur),
            name: "recur",
            shape: "(recur argument ...)",
        },
        KernelFormRow {
            form: KernelForm::Expression(ExpressionForm::Try),
            name: "try",
            shape: "(try expression)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Trait),
            name: "trait",
            shape: "(trait name (method type) ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Impl),
            name: "impl",
            shape: "(impl trait type (method (param ...) body ...) ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Use),
            name: "use",
            shape: "(use module name ...)",
        },
        KernelFormRow {
            form: KernelForm::Definition(DefinitionForm::Export),
            name: "export",
            shape: "(export name ...)",
        },
    ];

    pub fn all() -> impl Iterator<Item = KernelForm> {
        Self::ROWS.iter().map(|row| row.form)
    }

    pub fn name(self) -> &'static str {
        self.row().name
    }

    pub fn shape(self) -> &'static str {
        self.row().shape
    }

    fn row(self) -> &'static KernelFormRow {
        Self::ROWS
            .iter()
            .find(|row| row.form == self)
            .expect("every kernel form has a row in the kernel form table")
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

struct PrimitiveRow {
    primitive: PrimitiveType,
    name: &'static str,
    integer_range: Option<(i128, i128)>,
}

impl PrimitiveType {
    const ROWS: [PrimitiveRow; 19] = [
        PrimitiveRow {
            primitive: PrimitiveType::Bool,
            name: "bool",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::Char,
            name: "char",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::Str,
            name: "str",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::String,
            name: "string",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::Unit,
            name: "unit",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::I8,
            name: "i8",
            integer_range: Some((i8::MIN as i128, i8::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::I16,
            name: "i16",
            integer_range: Some((i16::MIN as i128, i16::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::I32,
            name: "i32",
            integer_range: Some((i32::MIN as i128, i32::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::I64,
            name: "i64",
            integer_range: Some((i64::MIN as i128, i64::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::I128,
            name: "i128",
            integer_range: Some((i128::MIN, i128::MAX)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::Isize,
            name: "isize",
            integer_range: Some((i64::MIN as i128, i64::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::U8,
            name: "u8",
            integer_range: Some((0, u8::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::U16,
            name: "u16",
            integer_range: Some((0, u16::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::U32,
            name: "u32",
            integer_range: Some((0, u32::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::U64,
            name: "u64",
            integer_range: Some((0, u64::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::U128,
            name: "u128",
            integer_range: Some((0, i128::MAX)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::Usize,
            name: "usize",
            integer_range: Some((0, u64::MAX as i128)),
        },
        PrimitiveRow {
            primitive: PrimitiveType::F32,
            name: "f32",
            integer_range: None,
        },
        PrimitiveRow {
            primitive: PrimitiveType::F64,
            name: "f64",
            integer_range: None,
        },
    ];

    pub fn all() -> impl Iterator<Item = PrimitiveType> {
        Self::ROWS.iter().map(|row| row.primitive)
    }

    pub fn name(self) -> &'static str {
        self.row().name
    }

    pub fn integer_range(self) -> Option<(i128, i128)> {
        self.row().integer_range
    }

    pub fn is_integer(self) -> bool {
        self.integer_range().is_some()
    }

    pub fn is_float(self) -> bool {
        matches!(self, PrimitiveType::F32 | PrimitiveType::F64)
    }

    fn row(self) -> &'static PrimitiveRow {
        Self::ROWS
            .iter()
            .find(|row| row.primitive == self)
            .expect("every primitive type has a row in the primitive table")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelType {
    Function(ClosureKind),
    Reference(ReferenceKind),
    Where,
    With,
    Hole,
    Primitive(PrimitiveType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    Shared,
    Mutable,
}

impl ReferenceKind {
    pub fn keyword(self) -> &'static str {
        match self {
            ReferenceKind::Shared => "ref",
            ReferenceKind::Mutable => "mut",
        }
    }
}

impl KernelType {
    pub fn all() -> impl Iterator<Item = KernelType> {
        [
            KernelType::Function(ClosureKind::Fn),
            KernelType::Function(ClosureKind::FnMut),
            KernelType::Function(ClosureKind::FnOnce),
            KernelType::Reference(ReferenceKind::Shared),
            KernelType::Reference(ReferenceKind::Mutable),
            KernelType::Where,
            KernelType::With,
            KernelType::Hole,
        ]
        .into_iter()
        .chain(PrimitiveType::all().map(KernelType::Primitive))
    }

    pub fn shape(self) -> &'static str {
        match self {
            KernelType::Function(kind) => kind.shape(),
            KernelType::Reference(ReferenceKind::Shared) => "(ref type)",
            KernelType::Reference(ReferenceKind::Mutable) => "(mut type)",
            KernelType::Where => "(where (variable (variable trait ...) ...) type)",
            KernelType::With => "(with type type)",
            KernelType::Hole | KernelType::Primitive(_) => self.name(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            KernelType::Function(kind) => kind.name(),
            KernelType::Reference(kind) => kind.keyword(),
            KernelType::Where => "where",
            KernelType::With => "with",
            KernelType::Hole => "_",
            KernelType::Primitive(primitive) => primitive.name(),
        }
    }
}

pub struct KernelConstants;

impl KernelConstants {
    pub const BOOLEANS: [(&'static str, bool); 2] = [("true", true), ("false", false)];
}

pub struct CopyTrait;

impl CopyTrait {
    pub const PATH: &'static str = "Copy";
}

pub struct OrderTrait;

impl OrderTrait {
    pub const PATH: &'static str = "Ord";
}

pub struct FunctionFieldTraits;

impl FunctionFieldTraits {
    pub const ALLOWED: [&'static str; 3] = ["Clone", "Copy", "Debug"];
}

pub struct StructDerives;

impl StructDerives {
    pub const COPY: &'static str = "Copy";
    pub const INFERRED: [&'static str; 6] = ["Clone", "Copy", "Debug", "PartialEq", "Eq", "Hash"];
    pub const DERIVABLE: [&'static str; 8] = [
        "Clone",
        "Copy",
        "Debug",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "Hash",
    ];
    const REQUIRED: [(&'static str, &'static str); 5] = [
        ("Copy", "Clone"),
        ("Eq", "PartialEq"),
        ("PartialOrd", "PartialEq"),
        ("Ord", "Eq"),
        ("Ord", "PartialOrd"),
    ];

    pub fn derive_name(path: &str) -> &str {
        path.rsplit("::").next().unwrap_or(path)
    }

    pub fn brings(listed: &str, derived: &str) -> bool {
        listed == derived
            || Self::REQUIRED
                .iter()
                .any(|(requirer, required)| *requirer == listed && Self::brings(required, derived))
    }
}
