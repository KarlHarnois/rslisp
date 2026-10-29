pub struct NameMangler;

impl NameMangler {
    const PUNCTUATION: [(char, &'static str); 23] = [
        ('-', "_"),
        ('+', "plus"),
        ('*', "star"),
        ('/', "slash"),
        ('<', "lt"),
        ('>', "gt"),
        ('=', "eq"),
        ('!', "bang"),
        ('?', "query"),
        ('%', "percent"),
        ('&', "and"),
        ('|', "or"),
        ('^', "caret"),
        ('~', "tilde"),
        ('.', "dot"),
        (':', "colon"),
        ('@', "at"),
        ('$', "dollar"),
        ('#', "hash"),
        ('\'', "prime"),
        (',', "comma"),
        ('\\', "backslash"),
        ('`', "backtick"),
    ];
    const LONE_SEPARATORS: [(char, &'static str); 2] = [('-', "minus"), ('_', "underscore")];
    const CODE_POINT_PREFIX: char = 'u';
    const WILDCARD: &str = "_";

    const PREDICATE_SUFFIX: char = '?';
    const CHANGE_SUFFIX: char = '!';
    const PREDICATE_PREFIX: &str = "is_";
    const ARROW_START: char = '-';
    const ARROW_END: char = '>';
    const ARROW_WORD: &str = "to";
    const STRICT_KEYWORDS: [&str; 35] = [
        "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
        "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "static", "struct", "trait", "true", "type", "unsafe",
        "use", "where", "while",
    ];
    const RESERVED_KEYWORDS: [&str; 13] = [
        "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof",
        "unsized", "virtual", "yield",
    ];
    const NON_RAW_KEYWORDS: [&str; 4] = ["self", "Self", "super", "crate"];
    const PRELUDE_NAMES: [&str; 34] = [
        "AsMut",
        "AsRef",
        "Box",
        "Clone",
        "Copy",
        "Default",
        "DoubleEndedIterator",
        "Drop",
        "Eq",
        "ExactSizeIterator",
        "Extend",
        "Fn",
        "FnMut",
        "FnOnce",
        "From",
        "FromIterator",
        "Into",
        "IntoIterator",
        "Iterator",
        "Option",
        "Ord",
        "PartialEq",
        "PartialOrd",
        "Result",
        "Send",
        "Sized",
        "String",
        "Sync",
        "ToOwned",
        "ToString",
        "TryFrom",
        "TryInto",
        "Unpin",
        "Vec",
    ];
    const PRELUDE_SUFFIX: char = '2';

    pub fn local(name: &str) -> String {
        if name == Self::WILDCARD {
            name.to_string()
        } else {
            Self::value(name)
        }
    }

    pub fn value(name: &str) -> String {
        Self::guard_keyword(Self::snake(Self::without_change_suffix(name)))
    }

    fn without_change_suffix(name: &str) -> &str {
        match name.strip_suffix(Self::CHANGE_SUFFIX) {
            Some(body) if !body.is_empty() => body,
            _ => name,
        }
    }

    pub fn constant(name: &str) -> String {
        Self::snake(name).to_uppercase()
    }

    pub fn lifetime(name: &str) -> String {
        let body = name.strip_prefix('\'').unwrap_or(name);
        let mangled = Self::snake(body);
        let reserved = mangled == "_"
            || mangled == "static"
            || Self::NON_RAW_KEYWORDS.contains(&mangled.as_str())
            || Self::STRICT_KEYWORDS.contains(&mangled.as_str())
            || Self::RESERVED_KEYWORDS.contains(&mangled.as_str());
        if reserved {
            format!("'{mangled}_")
        } else {
            format!("'{mangled}")
        }
    }

    pub fn type_name(name: &str) -> String {
        let name = Self::variant_name(name);
        if Self::PRELUDE_NAMES.contains(&name.as_str()) {
            format!("{name}{}", Self::PRELUDE_SUFFIX)
        } else {
            name
        }
    }

    pub fn variant_name(name: &str) -> String {
        Self::snake(name)
            .split('_')
            .filter(|word| !word.is_empty())
            .map(Self::capitalized)
            .collect()
    }

    fn snake(name: &str) -> String {
        let (body, is_predicate) = match name.strip_suffix(Self::PREDICATE_SUFFIX) {
            Some(body) if !body.is_empty() => (body, true),
            _ => (name, false),
        };
        let spelled = Self::spell(body);
        let prefixed = if is_predicate {
            format!("{}{spelled}", Self::PREDICATE_PREFIX)
        } else {
            Self::spelled_or_separators(spelled, body)
        };
        Self::valid_start(prefixed)
    }

    fn spelled_or_separators(spelled: String, name: &str) -> String {
        if spelled.is_empty() {
            Self::spell_separators(name)
        } else {
            spelled
        }
    }

    fn spell_separators(name: &str) -> String {
        name.chars()
            .filter_map(Self::separator_word)
            .collect::<Vec<_>>()
            .join("_")
    }

    fn separator_word(character: char) -> Option<&'static str> {
        Self::LONE_SEPARATORS
            .iter()
            .find(|(separator, _)| *separator == character)
            .map(|(_, word)| *word)
    }

    fn spell(name: &str) -> String {
        let mut spelled = String::new();
        let mut characters = name.chars().peekable();
        while let Some(character) = characters.next() {
            if character == Self::ARROW_START && characters.peek() == Some(&Self::ARROW_END) {
                characters.next();
                Self::push_word(&mut spelled, Self::ARROW_WORD);
            } else if let Some(word) = Self::spelling(character) {
                Self::push_word(&mut spelled, word);
            } else if Self::continues_identifier(character) {
                spelled.push(character);
            } else {
                Self::push_word(&mut spelled, &Self::code_point(character));
            }
        }
        spelled.trim_end_matches('_').to_string().replace("__", "_")
    }

    fn push_word(spelled: &mut String, word: &str) {
        if !spelled.is_empty() && !spelled.ends_with('_') && word != "_" {
            spelled.push('_');
        }
        spelled.push_str(word);
        if word != "_" {
            spelled.push('_');
        }
    }

    fn spelling(character: char) -> Option<&'static str> {
        Self::PUNCTUATION
            .iter()
            .find(|(punctuation, _)| *punctuation == character)
            .map(|(_, word)| *word)
    }

    fn continues_identifier(character: char) -> bool {
        character.is_ascii_digit() || character == '_' || character.is_alphabetic()
    }

    fn code_point(character: char) -> String {
        format!("{}{:x}", Self::CODE_POINT_PREFIX, u32::from(character))
    }

    fn valid_start(name: String) -> String {
        if !name.starts_with(|character: char| character == '_' || character.is_alphabetic()) {
            format!("_{name}")
        } else {
            name
        }
    }

    fn guard_keyword(name: String) -> String {
        if Self::NON_RAW_KEYWORDS.contains(&name.as_str()) {
            format!("{name}_")
        } else if Self::STRICT_KEYWORDS.contains(&name.as_str())
            || Self::RESERVED_KEYWORDS.contains(&name.as_str())
        {
            format!("r#{name}")
        } else {
            name
        }
    }

    fn capitalized(word: &str) -> String {
        let mut characters = word.chars();
        match characters.next() {
            Some(first) => first.to_uppercase().chain(characters).collect(),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::NameMangler;

    #[test]
    fn kebab_names_become_snake_case() {
        assert_eq!(NameMangler::value("make-point"), "make_point");
    }

    #[test]
    fn predicates_become_is_prefixed() {
        assert_eq!(NameMangler::value("admin?"), "is_admin");
        assert_eq!(NameMangler::constant("admin?"), "IS_ADMIN");
    }

    #[test]
    fn lifetimes_named_like_keywords_get_a_suffix() {
        assert_eq!(NameMangler::lifetime("'type"), "'type_");
        assert_eq!(NameMangler::lifetime("'static"), "'static_");
    }

    #[test]
    fn types_become_pascal_case() {
        assert_eq!(NameMangler::type_name("bank-account"), "BankAccount");
    }

    #[test]
    fn prelude_names_get_a_suffix() {
        assert_eq!(NameMangler::type_name("sized"), "Sized2");
        assert_eq!(NameMangler::type_name("send"), "Send2");
        assert_eq!(NameMangler::type_name("iterator"), "Iterator2");
    }

    #[test]
    fn punctuation_is_spelled_out() {
        assert_eq!(NameMangler::value("+"), "plus");
        assert_eq!(NameMangler::value("->string"), "to_string");
        assert_eq!(NameMangler::value("point->string"), "point_to_string");
        assert_eq!(NameMangler::value("set!"), "set");
        assert_eq!(NameMangler::value("set!x"), "set_bang_x");
    }

    #[test]
    fn every_printable_character_becomes_a_distinct_identifier() {
        let names: HashSet<String> = PrintableCharacters::ALL
            .map(|character| NameMangler::value(&character.to_string()))
            .inspect(|name| assert!(Identifier::is_valid(name), "`{name}` is not valid"))
            .collect();
        assert_eq!(names.len(), PrintableCharacters::ALL.count());
    }

    #[test]
    fn lone_separators_are_spelled_out() {
        assert_eq!(NameMangler::value("-"), "minus");
        assert_eq!(NameMangler::value("_"), "underscore");
        assert_eq!(NameMangler::value("_-"), "underscore_minus");
        assert_eq!(NameMangler::constant("_"), "UNDERSCORE");
    }

    #[test]
    fn a_wildcard_local_stays_a_wildcard() {
        assert_eq!(NameMangler::local("_"), "_");
        assert_eq!(NameMangler::local("-"), "minus");
    }

    #[test]
    fn characters_outside_identifiers_become_code_points() {
        assert_eq!(NameMangler::value("café"), "café");
        assert_eq!(NameMangler::value("☃"), "u2603");
        assert_eq!(NameMangler::value("snow☃man"), "snow_u2603_man");
    }

    struct PrintableCharacters;

    impl PrintableCharacters {
        const ALL: std::ops::RangeInclusive<char> = '!'..='~';
    }

    struct Identifier;

    impl Identifier {
        fn is_valid(name: &str) -> bool {
            let mut characters = name.chars();
            let starts = characters
                .next()
                .is_some_and(|first| first == '_' || first.is_alphabetic());
            starts && name != "_" && characters.all(|rest| rest == '_' || rest.is_alphanumeric())
        }
    }

    #[test]
    fn keywords_become_raw_identifiers() {
        for keyword in NameMangler::STRICT_KEYWORDS
            .into_iter()
            .chain(NameMangler::RESERVED_KEYWORDS)
        {
            assert_eq!(NameMangler::value(keyword), format!("r#{keyword}"));
        }
        for keyword in NameMangler::NON_RAW_KEYWORDS {
            assert_eq!(NameMangler::value(keyword), format!("{keyword}_"));
        }
    }
}
