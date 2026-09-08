def render(value: str) -> str:
    return value.upper()


class Formatter:
    def format(self, value: str) -> str:
        return render(value)
