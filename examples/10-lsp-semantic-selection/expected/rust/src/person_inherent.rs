impl Person {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
