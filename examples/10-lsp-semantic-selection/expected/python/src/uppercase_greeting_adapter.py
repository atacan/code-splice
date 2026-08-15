class UppercaseGreetingAdapter(GreetingAdapter):
    def greeting(self) -> str:
        return super().greeting().upper()
