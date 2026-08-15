from typing import Protocol

def keep() -> str:
    return "kept"
class Named(Protocol):
    name: str
class Person:
    def __init__(self, name: str) -> None:
        self.name = name
class GreetingAdapter:
    def __init__(self, subject: Named) -> None:
        self.subject = subject

    def greeting(self) -> str:
        return f"Hello, {self.subject.name}!"
class UppercaseGreetingAdapter(GreetingAdapter):
    def greeting(self) -> str:
        return super().greeting().upper()
