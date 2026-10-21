#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    First,
    Rest,
    List,
    Prepend,
    Concat,
    Length,
    IsEmpty,
    Nth,
    Take,
    Drop,
    IsSymbol,
    IsList,
    IsString,
    IsNumber,
    SymbolName,
    MakeSymbol,
    MakeString,
    SameBinding,
    Fail,
    NumberAdd,
    NumberSub,
    NumberMul,
    NumberDiv,
    NumberRem,
    NumberLt,
    NumberLe,
    NumberGt,
    NumberGe,
    ValueEq,
    BoolNot,
    BoolTrue,
    BoolFalse,
}

impl Builtin {
    const NAMES: [(Builtin, &'static str); 32] = [
        (Builtin::First, "first"),
        (Builtin::Rest, "rest"),
        (Builtin::List, "list"),
        (Builtin::Prepend, "prepend"),
        (Builtin::Concat, "concat"),
        (Builtin::Length, "length"),
        (Builtin::IsEmpty, "empty?"),
        (Builtin::Nth, "nth"),
        (Builtin::Take, "take"),
        (Builtin::Drop, "drop"),
        (Builtin::IsSymbol, "symbol?"),
        (Builtin::IsList, "list?"),
        (Builtin::IsString, "string?"),
        (Builtin::IsNumber, "number?"),
        (Builtin::SymbolName, "symbol-name"),
        (Builtin::MakeSymbol, "symbol"),
        (Builtin::MakeString, "string"),
        (Builtin::SameBinding, "same-binding?"),
        (Builtin::Fail, "fail"),
        (Builtin::NumberAdd, "number-add"),
        (Builtin::NumberSub, "number-sub"),
        (Builtin::NumberMul, "number-mul"),
        (Builtin::NumberDiv, "number-div"),
        (Builtin::NumberRem, "number-rem"),
        (Builtin::NumberLt, "number-lt"),
        (Builtin::NumberLe, "number-le"),
        (Builtin::NumberGt, "number-gt"),
        (Builtin::NumberGe, "number-ge"),
        (Builtin::ValueEq, "value-eq"),
        (Builtin::BoolNot, "bool-not"),
        (Builtin::BoolTrue, "bool-true"),
        (Builtin::BoolFalse, "bool-false"),
    ];

    pub fn all() -> impl Iterator<Item = Builtin> {
        Self::NAMES.iter().map(|(builtin, _)| *builtin)
    }

    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(builtin, _)| *builtin == self)
            .map(|(_, name)| *name)
            .expect("every builtin has a row in the name table")
    }
}
