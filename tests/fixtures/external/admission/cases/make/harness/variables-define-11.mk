
define FOO
@echo hello
echo world
endef
define BAR
echo hello
echo world
endef

all: foo bar
foo: ; $(FOO)
bar: ; @$(BAR)
