class GreetingAdapter:
    def __init__(self, subject: Named) -> None:
        self.subject = subject

    def greeting(self) -> str:
        return f"Hello, {self.subject.name}!"
