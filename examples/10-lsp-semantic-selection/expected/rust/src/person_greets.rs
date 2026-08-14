impl Greets for Person {
    fn greeting(&self) -> String {
        format!("Hello, {}!", self.name)
    }
}
