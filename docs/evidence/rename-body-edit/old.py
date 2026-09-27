import os
import sys

def calculate_total(items):
    total = 0
    for i in items:
        total += i.price * i.qty
    return min(total, 50)
