class Counter:
    value = 0

    def __init__(this, value: int):
        this.value = value

    def reset(instance):
        instance.value = 0

    def read(self):
        return self.value

    def unrelated(self, other):
        other.foreign = 1


class Labelled:
    @property
    def label(self) -> str:
        return "default"

    def rename(self, label: str):
        self.label = label


class Unsafe:
    @staticmethod
    def configure(target):
        target.static_only = 1


class Other:
    def __init__(self):
        self.value = "other"
