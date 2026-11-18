
foo: FOO = foo
bar: BAR = bar
foo: bar
bar: baz
baz: ; @echo $(FOO) $(BAR)
