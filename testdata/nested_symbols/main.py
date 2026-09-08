def first():
    def worker(item):
        return item

    return worker


def second():
    def worker(item):
        return item

    return worker


def factory():
    class Handler:
        def run(self, payload):
            return payload

    return Handler


def choose(flag):
    if flag:
        def selected(value):
            return value
    else:
        def selected(value):
            return value

    local = selected(1)
    transform = lambda value: value
    items = [item for item in range(2)]
    return local, transform, items
