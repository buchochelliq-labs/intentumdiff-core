
bar = Goodbye
foo::=$(bar)
bar = ${ugh}
ugh = Hello
all: ; @echo $(foo)
