from typing import Callable, TypeVar


Function = TypeVar("Function", bound=Callable[..., object])


def identity(function: Function) -> Function:
    return function


def replace(function: Function) -> object:
    return object()


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

    @classmethod
    def construct(cls, target):
        target.class_only = 1

    @replace
    def replaced(self, target):
        target.replaced_only = 1


class Other:
    def __init__(self):
        self.value = "other"


class Decorated:
    @identity
    def __init__(self, tag: str):
        self.tag = tag
