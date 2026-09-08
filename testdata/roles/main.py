import library
import library as lib
from library import Counter
from library import CONSTANT as NUMBER

value = NUMBER
value += 1
read = value
del value

item = Counter()
item.value = 0
item.value += lib.CONSTANT
del item.value

try:
    raise ValueError
except ValueError as error:
    caught = error

match {"value": 1}:
    case {"value": captured, **rest}:
        matched = captured
        remaining = rest

match item:
    case Counter(value=matched_value):
        final = matched_value
