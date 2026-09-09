"""Module documentation."""

from typing import overload

SETTING: int = 1
"""Attribute documentation."""

type Result[T] = list[T]


class Example(object):
    """Class documentation."""

    @staticmethod
    async def fetch(
        value: int,
        enabled: bool = True,
    ) -> str:
        """Async method documentation."""
        return str(value) if enabled else ""


@overload
def choose(value: int) -> int:
    """First overload documentation."""
    ...


@overload
def choose(value: str) -> str:
    """Second overload documentation."""
    ...


def choose(value):
    """Implementation documentation."""
    return value


class WithProperty:
    @property
    def value(self) -> int:
        """Getter documentation."""
        return 1

    @value.setter
    def value(self, new: int) -> None:
        """Setter documentation."""


def outer() -> int:
    """Outer documentation."""

    def inner(value: int) -> int:
        """Nested documentation."""
        return value

    return inner(1)


def transparent(function):
    return function


class OrdinaryMethods:
    def method(self) -> None:
        """Ordinary method documentation."""

    @staticmethod
    def utility() -> None:
        """Static method documentation."""

    @transparent
    def decorated(self) -> None:
        """Decorated method documentation."""
