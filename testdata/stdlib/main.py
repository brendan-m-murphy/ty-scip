from pathlib import Path
from _typeshed import SupportsLenAndGetItem

path = Path("example.txt")
size = len([1, 2, 3])
handle = open("example.txt", encoding="utf-8")


def first(items: SupportsLenAndGetItem[str]) -> str:
    return items[0]
