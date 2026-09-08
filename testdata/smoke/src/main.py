from library import Formatter, render as show

message = "module"


def run() -> str:
    message = "local"
    formatter = Formatter()
    return formatter.format(show(message))
