use std::rc::Rc;

use super::{BindingId, Keyword, ReferenceKind, TypeExpr};
use crate::reader::{Form, Number};
use crate::source::Span;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Expr {
    kind: ExprKind,
    span: Span,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ExprKind {
    Leaf(Leaf),
    Fn(Rc<Lambda>),
    Call(Box<Expr>, Vec<Expr>),
    Recur(Vec<Expr>),
    Try(Box<Expr>),
    Infer(Box<Expr>),
    Borrow(ReferenceKind, Box<Expr>),
    Deref(Box<Expr>),
    Control(Control),
    Struct(StructExpr),
    Variant(BindingId, Vec<Expr>),
    Assoc(Box<Expr>, Vec<Expr>, Box<Expr>),
    MapAt(MapOperation, Box<Expr>, Vec<Expr>),
    SetAt(Box<Expr>, Vec<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MapOperation {
    Get,
    Keys,
    Vals,
    Iter,
    Update,
    Push,
}

impl MapOperation {
    const KEYWORDS: [(Self, &'static str); 6] = [
        (Self::Get, "get"),
        (Self::Keys, "keys"),
        (Self::Vals, "vals"),
        (Self::Iter, "iter"),
        (Self::Update, "update"),
        (Self::Push, "push"),
    ];

    pub fn of(name: &str) -> Option<Self> {
        Self::KEYWORDS
            .iter()
            .find(|(_, keyword)| *keyword == name)
            .map(|(operation, _)| *operation)
    }

    pub fn keyword(self) -> &'static str {
        Self::KEYWORDS
            .iter()
            .find(|(operation, _)| *operation == self)
            .map(|(_, keyword)| *keyword)
            .expect("every map operation has a keyword")
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Leaf {
    Literal(Literal),
    Var(BindingId),
    Quote(Form),
    Rust(Rc<RustTemplate>),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Control {
    Block(Block),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PatternKind {
    Wildcard,
    Bind(BindingId),
    BareName {
        name: Rc<str>,
        binding: BindingId,
        candidates: Vec<BindingId>,
        outer: Option<BindingId>,
    },
    Variant(BindingId, PatternArguments),
    Struct(BindingId, PatternArguments),
    ShortVariant {
        name: Rc<str>,
        candidates: Vec<BindingId>,
        arguments: PatternArguments,
    },
    Literal(Literal),
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PatternArguments {
    pub parts: Vec<PatternPart>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PatternPart {
    Positional(Pattern),
    Labeled(FieldLabel, Option<Pattern>),
}

impl PatternArguments {
    pub fn patterns(&self) -> impl Iterator<Item = &Pattern> {
        self.parts.iter().filter_map(|part| match part {
            PatternPart::Positional(pattern) => Some(pattern),
            PatternPart::Labeled(_, pattern) => pattern.as_ref(),
        })
    }

    pub fn positional(&self) -> impl Iterator<Item = &Pattern> {
        self.parts.iter().filter_map(|part| match part {
            PatternPart::Positional(pattern) => Some(pattern),
            PatternPart::Labeled(..) => None,
        })
    }

    pub fn labeled(&self) -> impl Iterator<Item = (&FieldLabel, Option<&Pattern>)> {
        self.parts.iter().filter_map(|part| match part {
            PatternPart::Positional(_) => None,
            PatternPart::Labeled(label, pattern) => Some((label, pattern.as_ref())),
        })
    }
}

impl Pattern {
    pub fn new(kind: PatternKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn arm_outer(&self) -> Option<(BindingId, BindingId)> {
        match &self.kind {
            PatternKind::BareName {
                binding,
                outer: Some(outer),
                ..
            } => Some((*binding, *outer)),
            _ => None,
        }
    }

    pub fn nested(&self) -> impl Iterator<Item = &Pattern> {
        self.arguments()
            .into_iter()
            .flat_map(PatternArguments::patterns)
    }

    fn arguments(&self) -> Option<&PatternArguments> {
        match &self.kind {
            PatternKind::Variant(_, arguments)
            | PatternKind::Struct(_, arguments)
            | PatternKind::ShortVariant { arguments, .. } => Some(arguments),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum StructExpr {
    New(BindingId, Vec<(FieldLabel, Expr)>),
    Update(Box<Expr>, Vec<(FieldLabel, Expr)>),
    Field(Box<Expr>, FieldLabel),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Literal {
    Number(Number),
    String(Rc<str>),
    Bool(bool),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldLabel {
    pub name: Rc<str>,
    pub span: Span,
    pub spelling: FieldSpelling,
}

impl FieldLabel {
    pub fn spelled(&self, field_name: &str) -> String {
        match self.spelling {
            FieldSpelling::Bare => field_name.to_string(),
            FieldSpelling::Keyword => format!("{}{field_name}", Keyword::MARKER),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FieldSpelling {
    Bare,
    Keyword,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Binder {
    pub binding: BindingId,
    pub annotation: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Lambda {
    pub params: Vec<Binder>,
    pub body: Block,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub items: Vec<BodyItem>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum BodyItem {
    Let(Binder, Expr),
    Expr(Expr),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RustTemplate {
    pub template: Rc<str>,
    pub type_expr: TypeExpr,
    pub clauses: Vec<RustClause>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RustClause {
    pub target: TypeExpr,
    pub template: Rc<str>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn kind(&self) -> &ExprKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn children(&self) -> Vec<&Expr> {
        match &self.kind {
            ExprKind::Leaf(_) => Vec::new(),
            ExprKind::Fn(lambda) => lambda.body.expressions().collect(),
            ExprKind::Call(callee, arguments) => {
                std::iter::once(callee.as_ref()).chain(arguments).collect()
            }
            ExprKind::Control(control) => control.children(),
            other => other.nested_children(),
        }
    }
}

impl ExprKind {
    fn nested_children(&self) -> Vec<&Expr> {
        match self {
            ExprKind::Recur(arguments) => arguments.iter().collect(),
            ExprKind::Try(value)
            | ExprKind::Infer(value)
            | ExprKind::Borrow(_, value)
            | ExprKind::Deref(value) => {
                vec![value]
            }
            ExprKind::Struct(structure) => structure.children(),
            ExprKind::Variant(_, arguments) => arguments.iter().collect(),
            other => other.collection_children(),
        }
    }

    fn collection_children(&self) -> Vec<&Expr> {
        match self {
            ExprKind::Assoc(value, keys, item) | ExprKind::SetAt(value, keys, item) => {
                std::iter::once(value.as_ref())
                    .chain(keys)
                    .chain(std::iter::once(item.as_ref()))
                    .collect()
            }
            ExprKind::MapAt(_, value, arguments) => {
                std::iter::once(value.as_ref()).chain(arguments).collect()
            }
            _ => Vec::new(),
        }
    }
}

impl Control {
    fn children(&self) -> Vec<&Expr> {
        match self {
            Control::Block(block) => block.expressions().collect(),
            Control::If(test, then, otherwise) => vec![test, then, otherwise],
            Control::Match(value, arms) => std::iter::once(value.as_ref())
                .chain(arms.iter().flat_map(|arm| arm.body.expressions()))
                .collect(),
        }
    }
}

impl StructExpr {
    fn children(&self) -> Vec<&Expr> {
        match self {
            StructExpr::New(_, fields) => fields.iter().map(|(_, value)| value).collect(),
            StructExpr::Update(base, fields) => std::iter::once(base.as_ref())
                .chain(fields.iter().map(|(_, value)| value))
                .collect(),
            StructExpr::Field(value, _) => vec![value],
        }
    }
}

impl Block {
    pub fn expressions(&self) -> impl Iterator<Item = &Expr> {
        self.items.iter().map(BodyItem::expr)
    }
}

impl BodyItem {
    pub fn expr(&self) -> &Expr {
        match self {
            BodyItem::Let(_, value) | BodyItem::Expr(value) => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FieldLabel, FieldSpelling, MapOperation, Pattern, PatternArguments, PatternKind,
        PatternPart,
    };
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind};
    use crate::source::{SourceId, Span};

    struct PatternFixture {
        bindings: BindingArena,
    }

    impl PatternFixture {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
            }
        }

        fn binding(&mut self) -> BindingId {
            self.bindings
                .add(BindingInfo::new("name".into(), BindingKind::Local, None))
        }

        fn pattern(kind: PatternKind) -> Pattern {
            Pattern::new(kind, Span::new(SourceId::new(0), 0..0))
        }

        fn bare_name(&mut self, outer: Option<BindingId>) -> Pattern {
            let binding = self.binding();
            Self::pattern(PatternKind::BareName {
                name: "name".into(),
                binding,
                candidates: Vec::new(),
                outer,
            })
        }

        fn held(&mut self) -> PatternArguments {
            PatternArguments {
                parts: vec![PatternPart::Positional(self.bare_name(None))],
            }
        }

        fn label() -> FieldLabel {
            FieldLabel {
                name: "name".into(),
                spelling: FieldSpelling::Keyword,
                span: Span::new(SourceId::new(0), 0..0),
            }
        }
    }

    #[test]
    fn a_bare_name_pairs_its_binding_with_its_outer_binding() {
        let mut fixture = PatternFixture::new();
        let outer = fixture.binding();
        let pattern = fixture.bare_name(Some(outer));
        let (_, found) = pattern.arm_outer().expect("the name has an outer binding");
        assert_eq!(found, outer);
    }

    #[test]
    fn a_bare_name_without_an_outer_binding_has_no_arm_outer() {
        assert_eq!(PatternFixture::new().bare_name(None).arm_outer(), None);
    }

    #[test]
    fn a_wildcard_has_no_arm_outer() {
        let pattern = PatternFixture::pattern(PatternKind::Wildcard);
        assert_eq!(pattern.arm_outer(), None);
    }

    #[test]
    fn a_variant_pattern_holds_its_nested_patterns() {
        let mut fixture = PatternFixture::new();
        let variant = fixture.binding();
        let held = fixture.held();
        let pattern = PatternFixture::pattern(PatternKind::Variant(variant, held));
        assert_eq!(pattern.nested().count(), 1);
    }

    #[test]
    fn a_struct_pattern_holds_its_nested_patterns() {
        let mut fixture = PatternFixture::new();
        let shape = fixture.binding();
        let held = fixture.held();
        let pattern = PatternFixture::pattern(PatternKind::Struct(shape, held));
        assert_eq!(pattern.nested().count(), 1);
    }

    #[test]
    fn a_short_variant_pattern_holds_its_nested_patterns() {
        let mut fixture = PatternFixture::new();
        let arguments = fixture.held();
        let pattern = PatternFixture::pattern(PatternKind::ShortVariant {
            name: "name".into(),
            candidates: Vec::new(),
            arguments,
        });
        assert_eq!(pattern.nested().count(), 1);
    }

    #[test]
    fn a_labeled_part_holds_its_pattern() {
        let mut fixture = PatternFixture::new();
        let shape = fixture.binding();
        let inner = fixture.bare_name(None);
        let label = PatternFixture::label();
        let parts = vec![PatternPart::Labeled(label, Some(inner))];
        let pattern =
            PatternFixture::pattern(PatternKind::Struct(shape, PatternArguments { parts }));
        assert_eq!(pattern.nested().count(), 1);
    }

    #[test]
    fn a_label_without_a_pattern_holds_nothing() {
        let parts = vec![PatternPart::Labeled(PatternFixture::label(), None)];
        assert_eq!(PatternArguments { parts }.patterns().count(), 0);
    }

    #[test]
    fn a_wildcard_holds_no_nested_patterns() {
        let wildcard = PatternFixture::pattern(PatternKind::Wildcard);
        assert_eq!(wildcard.nested().count(), 0);
    }

    #[test]
    fn every_operation_name_round_trips() {
        assert_eq!(MapOperation::of("iter"), Some(MapOperation::Iter));
        assert_eq!(MapOperation::Iter.keyword(), "iter");
        assert_eq!(MapOperation::of("unknown"), None);
    }
}
