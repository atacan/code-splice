pub fn keep() -> &'static str {
    "kept"
}
pub trait Greets {
    fn greeting(&self) -> String;
}
pub struct Person {
    pub name: String,
}
impl Person {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
impl Greets for Person {
    fn greeting(&self) -> String {
        format!("Hello, {}!", self.name)
    }
}
