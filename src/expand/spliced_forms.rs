use std::collections::VecDeque;

use crate::reader::Form;

#[derive(Default)]
pub struct SplicedForms {
    forms: VecDeque<SplicedForm>,
}

pub struct SplicedForm {
    pub form: Form,
    pub depth: usize,
}

impl SplicedForms {
    pub fn splice(&mut self, forms: &[Form], depth: usize) {
        for form in forms.iter().rev() {
            let form = form.clone();
            self.forms.push_front(SplicedForm { form, depth });
        }
    }

    pub fn take_first(&mut self) -> Option<SplicedForm> {
        self.forms.pop_front()
    }
}
