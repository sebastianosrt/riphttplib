#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: Option<String>,
}

impl Header {
    pub fn new(name: String, value: String) -> Self { // TODO consider using &str
        Self {
            name: name, // TODO make sure in http2/3 headers are lowercase
            value: Some(value),
        }
    }

    pub fn new_valueless(name: String) -> Self {
        Self { name: name, value: None }
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
