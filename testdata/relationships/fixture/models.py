from .base import Base, Left, Right


class Child(Base):
    pass


class Mixed(Left, Right):
    pass


class Meta(type):
    pass


class WithMeta(Base, metaclass=Meta):
    pass
