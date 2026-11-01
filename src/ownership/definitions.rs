use std::collections::HashSet;
use std::rc::Rc;

use super::lambda_address::LambdaAddress;
use crate::core::BindingId;
use crate::scope::FastHashMap;
use crate::types::{TypedExpr, TypedExprKind, TypedLambda};

#[derive(Default)]
pub struct LocalDefinitions {
    collected: FastHashMap<LambdaAddress, Rc<HashSet<BindingId>>>,
}

impl LocalDefinitions {
    pub fn of(&mut self, lambda: &TypedLambda) -> Rc<HashSet<BindingId>> {
        let address = LambdaAddress::of(lambda);
        if let Some(bindings) = self.collected.get(&address) {
            return Rc::clone(bindings);
        }
        let bindings = Rc::new(self.collect(lambda));
        self.collected.insert(address, Rc::clone(&bindings));
        bindings
    }

    pub fn forget(&mut self) {
        self.collected.clear();
    }

    fn collect(&mut self, lambda: &TypedLambda) -> HashSet<BindingId> {
        let mut bindings = lambda.binders().map(|binder| binder.binding).collect();
        for expr in lambda.body.expressions() {
            self.expr(expr, &mut bindings);
        }
        bindings
    }

    fn expr(&mut self, expr: &TypedExpr, bindings: &mut HashSet<BindingId>) {
        if let TypedExprKind::Fn(lambda) = &expr.kind {
            bindings.extend(self.of(lambda).iter());
            return;
        }
        bindings.extend(expr.binders().into_iter().map(|binder| binder.binding));
        for child in expr.children() {
            self.expr(child, bindings);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::rc::Rc;

    use super::LocalDefinitions;
    use crate::core::BindingId;
    use crate::ownership::lambda_address::LambdaAddress;
    use crate::ownership::lambda_source::LambdaSource;

    struct DefinitionFixture {
        source: LambdaSource,
        definitions: LocalDefinitions,
    }

    impl DefinitionFixture {
        const NESTED_LAMBDAS: &str = "
            (defn main ()
              (let model \"Leaf\")
              (let describe (fn (make)
                (let label (fn (year) year))
                (label 2024)))
              (describe \"Nissan\"))";

        fn new() -> Self {
            Self {
                source: LambdaSource::new(Self::NESTED_LAMBDAS),
                definitions: LocalDefinitions::default(),
            }
        }

        fn names_defined_in(&mut self, name: &str) -> HashSet<&str> {
            let bindings = self.definitions_of(name);
            self.source.names(&bindings)
        }

        fn definitions_of(&mut self, name: &str) -> Rc<HashSet<BindingId>> {
            self.definitions.of(self.source.lambda(name))
        }

        fn store(&mut self, lambda: &str, bindings: &[&str]) {
            let stored = bindings
                .iter()
                .map(|name| self.source.binder(name).binding)
                .collect();
            let address = LambdaAddress::of(self.source.lambda(lambda));
            self.definitions.collected.insert(address, Rc::new(stored));
        }
    }

    #[test]
    fn a_lambda_defines_the_bindings_of_each_lambda_inside_it() {
        let mut fixture = DefinitionFixture::new();
        let defined = fixture.names_defined_in("describe");
        assert_eq!(defined, HashSet::from(["make", "label", "year"]));
    }

    #[test]
    fn a_lambda_reads_the_bindings_already_collected_for_a_lambda_inside_it() {
        let mut fixture = DefinitionFixture::new();
        fixture.store("label", &["model"]);
        let defined = fixture.names_defined_in("describe");
        assert_eq!(defined, HashSet::from(["make", "label", "model"]));
    }

    #[test]
    fn a_lambda_asked_for_twice_is_collected_once() {
        let mut fixture = DefinitionFixture::new();
        let first = fixture.definitions_of("describe");
        let second = fixture.definitions_of("describe");
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_lambda_asked_for_after_forgetting_is_collected_again() {
        let mut fixture = DefinitionFixture::new();
        let first = fixture.definitions_of("describe");
        fixture.definitions.forget();
        let second = fixture.definitions_of("describe");
        assert!(!Rc::ptr_eq(&first, &second));
    }
}
