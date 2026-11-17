
W = bad
X = bad
foo: W = ok
foo:: ; @echo $(W) $(X) $(Y) $(Z)
foo:: ; @echo $(W) $(X) $(Y) $(Z)
foo: X = ok

Y = foo
bar: foo
bar: Y = bar

Z = nopat
ifdef PATTERN
  fo% : Z = pat
endif
