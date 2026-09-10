import sys
import os

def _subtotal(i):
    return i.price * i.qty

def compute_order_total(items):
    total = 0
    for i in items:
        total += _subtotal(i)
    return min(total, 75)
