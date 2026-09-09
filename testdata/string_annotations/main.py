from typing import Literal


class Node:
    pass


class Container:
    __slots__ = ("Node",)


def connect(
    parent: "Node | list[Node]",
    child: list["Node"],
    label: Literal["Node"] = "Node",
    joined: "No" "de" = None,
) -> "Node":
    return child[0]
