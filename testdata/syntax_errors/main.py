def before(value: int) -> int:
    return value + 1


broken =


def after(value: int) -> int:
    return before(value)


result = after(1)
