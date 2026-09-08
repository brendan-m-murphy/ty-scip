"""Module documentation."""

from typing import overload

SETTING: int = 1
"""Attribute documentation."""


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

