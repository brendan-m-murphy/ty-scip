from typing import NamedTuple, TypedDict


class Movie(TypedDict):
    title: str
    year: int


class Point(NamedTuple):
    x: int
    y: int


movie = Movie(title="Arrival", year=2016)
selected_title = movie["title"]

ordinary = {"title": "other"}
selected_ordinary = ordinary["title"]

point = Point(x=1, y=2)
