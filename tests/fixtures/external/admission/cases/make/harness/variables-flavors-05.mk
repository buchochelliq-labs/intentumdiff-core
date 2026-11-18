
ugh = Goodbye
bar = ${ugh}
foo ?= $(bar)
ugh = Hello
all: ; @echo $(foo)
