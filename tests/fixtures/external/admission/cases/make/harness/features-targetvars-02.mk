
.PHONY: all one
all: FOO += baz
all: one; @echo $(FOO)

FOO = bar

one: FOO += biz
one: FOO += boz
one: ; @echo $(FOO)
