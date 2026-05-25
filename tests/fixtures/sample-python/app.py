"""Caller of lib.py — exercises cross-file edges in later phases."""

from lib import Counter, double


def main():
    c = Counter()
    c.bump()
    print(double(c.count))
