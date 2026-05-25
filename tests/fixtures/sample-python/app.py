"""Caller of lib.py — exercises cross-file edges in later phases."""

from lib import Counter, double


def main():
    c = Counter()
    c.bump()
    print(double(c.count))


def risky_eval(expr):
    # Triggers python-eval-use rule.
    return eval(expr)


def risky_exec(code):
    # Triggers python-exec-use rule.
    exec(code)
