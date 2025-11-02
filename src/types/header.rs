#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: Option<String>,
}

impl Header {
    pub fn new(name: String, value: String) -> Self {
        // TODO consider using &str
        Self {
            name: name,
            value: Some(value),
        }
    }

    pub fn new_valueless(name: String) -> Self {
        Self {
            name: name,
            value: None,
        }
    }

    pub fn normalize(&mut self) -> &mut Self {
        self.name.make_ascii_lowercase();
        self
    }

    pub fn to_string(&self) -> String {
        if let Some(ref value) = self.value {
            format!("{}: {}", self.name, value)
        } else {
            self.name.clone()
        }
    }
}

impl std::fmt::Display for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref value) = self.value {
            write!(f, "{}: {}", self.name, value)
        } else {
            write!(f, "{}", self.name)
        }
    }
}
