
ugh = Goodbye
foo = $(bar)
bar = ${ugh}
# final value
ugh = Hello
all: ; @echo $(foo)
