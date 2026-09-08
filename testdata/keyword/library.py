def time_offset(period=None):
    transform = lambda value: value
    return transform(period)


class Box:
    def __init__(self):
        self.value = 1

    def get(self):
        return self.value
