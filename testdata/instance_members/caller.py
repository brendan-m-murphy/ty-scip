from library import Counter, Decorated, Labelled, Other


counter = Counter(1)
counter.value


class Child(Counter):
    def inherited(self):
        return self.value


labelled = Labelled()
labelled.label


def ambiguous(item: Counter | Other):
    return item.value


decorated = Decorated("ready")
decorated.tag
