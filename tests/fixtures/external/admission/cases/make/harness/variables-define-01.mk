
FOO = foo

define multi =
echo hi
@echo $(FOO)
endef # this is the end

define simple :=
@echo $(FOO)
endef

define posix ::=
@echo $(FOO)
endef

append = @echo a

define append +=

@echo b
endef

define cond ?= # this is a conditional
@echo first
endef

define cond ?=
@echo second
endef

FOO = there

all: ; $(multi)
	$(simple)
	$(posix)
	$(append)
	$(cond)
