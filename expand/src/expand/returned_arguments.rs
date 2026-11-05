use rustc_hash::FxHashMap;

use crate::reader::{Form, FormKind};

pub struct ReturnedArguments {
    call: Form,
}

impl ReturnedArguments {
    const LINEAR_SEARCH_LIMIT: usize = 8;

    pub fn of_call(call: &Form) -> Self {
        Self { call: call.clone() }
    }

    pub fn into_form_count(self, expansion: &Form) -> usize {
        let arguments = self.arguments();
        let elements = expansion.as_list().unwrap_or_default();
        if arguments.len() <= Self::LINEAR_SEARCH_LIMIT {
            Self::count_by_search(arguments, elements)
        } else {
            Self::count_by_set(arguments, elements)
        }
    }

    fn arguments(&self) -> &[Form] {
        self.call
            .as_list()
            .and_then(|items| items.get(1..))
            .unwrap_or_default()
    }

    fn count_by_search(arguments: &[Form], elements: &[Form]) -> usize {
        let mut returned = [false; Self::LINEAR_SEARCH_LIMIT];
        elements
            .iter()
            .filter(|element| Self::mark_returned(arguments, &mut returned, element))
            .map(Form::form_count)
            .sum()
    }

    fn mark_returned(arguments: &[Form], returned: &mut [bool], element: &Form) -> bool {
        let unreturned_match = arguments
            .iter()
            .zip(returned.iter_mut())
            .find(|(argument, returned)| !**returned && argument.is_same_form(element));
        match unreturned_match {
            Some((_, returned)) => {
                *returned = true;
                true
            }
            None => false,
        }
    }

    fn count_by_set(arguments: &[Form], elements: &[Form]) -> usize {
        let mut unreturned: FxHashMap<*const FormKind, usize> = FxHashMap::default();
        for argument in arguments {
            *unreturned.entry(argument.identity()).or_default() += 1;
        }
        elements
            .iter()
            .filter(|element| Self::take_unreturned(&mut unreturned, element))
            .map(Form::form_count)
            .sum()
    }

    fn take_unreturned(unreturned: &mut FxHashMap<*const FormKind, usize>, element: &Form) -> bool {
        match unreturned.get_mut(&element.identity()) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReturnedArguments;
    use crate::reader::{Form, FormKind, Symbol};
    use crate::source::{SourceId, Span};

    struct CallFixture;

    impl CallFixture {
        const MANY_ARGUMENTS: usize = ReturnedArguments::LINEAR_SEARCH_LIMIT + 1;

        fn call_of(arguments: &[Form]) -> Form {
            Self::list_of([vec![Self::symbol("head")], arguments.to_vec()].concat())
        }

        fn arguments(count: usize) -> Vec<Form> {
            (0..count)
                .map(|_| Self::list_of(vec![Self::symbol("leaf")]))
                .collect()
        }

        fn list_of(items: Vec<Form>) -> Form {
            Form::new(FormKind::List(items), Self::span())
        }

        fn symbol(name: &str) -> Form {
            Form::new(FormKind::Symbol(Symbol::new(name)), Self::span())
        }

        fn counted(arguments: &[Form], expansion: Vec<Form>) -> usize {
            let call = Self::call_of(arguments);
            ReturnedArguments::of_call(&call).into_form_count(&Self::list_of(expansion))
        }

        fn span() -> Span {
            Span::new(SourceId::new(0), 0..0)
        }
    }

    #[test]
    fn counts_the_forms_of_a_returned_argument() {
        let arguments = CallFixture::arguments(2);
        let expansion = vec![CallFixture::symbol("new"), arguments[1].clone()];
        assert_eq!(CallFixture::counted(&arguments, expansion), 2);
    }

    #[test]
    fn counts_no_form_that_the_expansion_built() {
        let arguments = CallFixture::arguments(2);
        let expansion = vec![CallFixture::list_of(vec![CallFixture::symbol("leaf")])];
        assert_eq!(CallFixture::counted(&arguments, expansion), 0);
    }

    #[test]
    fn counts_an_argument_returned_twice_once() {
        let arguments = CallFixture::arguments(1);
        let expansion = vec![arguments[0].clone(), arguments[0].clone()];
        assert_eq!(CallFixture::counted(&arguments, expansion), 2);
    }

    #[test]
    fn counts_a_returned_argument_among_many() {
        let arguments = CallFixture::arguments(CallFixture::MANY_ARGUMENTS);
        let last = arguments[CallFixture::MANY_ARGUMENTS - 1].clone();
        let expansion = vec![last, CallFixture::symbol("new")];
        assert_eq!(CallFixture::counted(&arguments, expansion), 2);
    }

    #[test]
    fn counts_a_form_passed_as_two_arguments_among_many_twice() {
        let mut arguments = CallFixture::arguments(CallFixture::MANY_ARGUMENTS);
        arguments[1] = arguments[0].clone();
        let expansion = vec![arguments[0].clone(), arguments[0].clone()];
        assert_eq!(CallFixture::counted(&arguments, expansion), 4);
    }

    #[test]
    fn counts_a_form_passed_as_two_arguments_twice() {
        let mut arguments = CallFixture::arguments(2);
        arguments[1] = arguments[0].clone();
        let expansion = vec![arguments[0].clone(), arguments[0].clone()];
        assert_eq!(CallFixture::counted(&arguments, expansion), 4);
    }

    #[test]
    fn counts_an_argument_returned_twice_among_many_once() {
        let arguments = CallFixture::arguments(CallFixture::MANY_ARGUMENTS);
        let expansion = vec![arguments[3].clone(), arguments[3].clone()];
        assert_eq!(CallFixture::counted(&arguments, expansion), 2);
    }
}
