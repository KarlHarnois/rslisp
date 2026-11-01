use std::collections::VecDeque;

use crate::reader::Form;

#[derive(Default)]
pub struct SplicedForms {
    forms: VecDeque<SplicedForm>,
    taken_splices: usize,
}

pub struct SplicedForm {
    pub form: Form,
    pub splices: usize,
}

impl SplicedForms {
    pub fn splice_into_taken(&mut self, forms: &[Form]) {
        let splices = self.taken_splices + 1;
        for form in forms.iter().rev() {
            let form = form.clone();
            self.forms.push_front(SplicedForm { form, splices });
        }
    }

    pub fn take_first(&mut self) -> Option<SplicedForm> {
        let first = self.forms.pop_front()?;
        self.taken_splices = first.splices;
        Some(first)
    }
}
