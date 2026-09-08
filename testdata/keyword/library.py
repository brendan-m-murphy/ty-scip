from typing import overload


def time_offset(period=None):
    transform = lambda value: value
    return transform(period)


@overload
def process(data: str, format: str) -> str: ...


@overload
def process(data: int, format: int) -> int: ...


def process(data, format):
    return data


class Box:
    def __init__(self):
        self.value = 1

    def get(self):
        return self.value
