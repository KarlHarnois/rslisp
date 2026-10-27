use std::cell::RefCell;

pub struct FreshLifetimes {
    reserved: Vec<String>,
    issued: RefCell<Vec<String>>,
}

impl FreshLifetimes {
    const LETTERS: usize = 26;

    pub fn new(reserved: Vec<String>) -> Self {
        Self {
            reserved,
            issued: RefCell::new(Vec::new()),
        }
    }

    pub fn issue(&self) -> String {
        let mut used = self.reserved.clone();
        used.extend(self.issued.borrow().iter().cloned());
        let name = Self::unused_name(&used);
        self.issued.borrow_mut().push(name.clone());
        name
    }

    pub fn issued(&self) -> Vec<String> {
        self.issued.borrow().clone()
    }

    pub fn unused_name(used: &[String]) -> String {
        let mut position = 0;
        loop {
            let name = if position < Self::LETTERS {
                format!(
                    "'{}",
                    char::from(b'a' + u8::try_from(position).expect("fewer than 26"))
                )
            } else {
                format!("'a{position}")
            };
            if !used.contains(&name) {
                return name;
            }
            position += 1;
        }
    }
}
