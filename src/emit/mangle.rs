pub struct NameMangler;

impl NameMangler {
    const PUNCTUATION: [(char, &'static str); 20] = [
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
    ];

    const PREDICATE_SUFFIX: char = '?';
    const CHANGE_SUFFIX: char = '!';
    const PREDICATE_PREFIX: &str = "is_";
    const ARROW_START: char = '-';
    const ARROW_END: char = '>';
    const ARROW_WORD: &str = "to";
    const STRICT_KEYWORDS: [&str; 39] = [
        "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
        "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
        "pub", "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use",
        "where", "while", "abstract", "become", "box", "do", "gen",
    ];
    const NON_RAW_KEYWORDS: [&str; 4] = ["self", "Self", "super", "crate"];

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

    pub fn type_name(name: &str) -> String {
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
            spelled
        };
        Self::valid_start(prefixed)
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
            } else {
                spelled.push(character);
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

    fn valid_start(name: String) -> String {
        if name.starts_with(|character: char| character.is_ascii_digit()) || name.is_empty() {
            format!("_{name}")
        } else {
            name
        }
    }

    fn guard_keyword(name: String) -> String {
        if Self::NON_RAW_KEYWORDS.contains(&name.as_str()) {
            format!("{name}_")
        } else if Self::STRICT_KEYWORDS.contains(&name.as_str()) {
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
    fn types_become_pascal_case() {
        assert_eq!(NameMangler::type_name("bank-account"), "BankAccount");
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
    fn keywords_become_raw_identifiers() {
        assert_eq!(NameMangler::value("type"), "r#type");
        assert_eq!(NameMangler::value("self"), "self_");
    }
}
