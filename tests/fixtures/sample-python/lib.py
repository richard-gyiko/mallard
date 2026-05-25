"""Minimal Python fixture for A1 scaffolding. Symbols + calls land in A2/A3."""


def double(x):
    return x * 2


class Counter:
    def __init__(self):
        self.count = 0

    def bump(self):
        self.count += 1
        return double(self.count)
