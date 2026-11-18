
VPATH = t1
include rules.mk
.PHONY: all
all: foo.x
foo.x : rules.mk ; @echo MYVAR=$(MYVAR) FOOVAR=$(FOOVAR) ALLVAR=$(ALLVAR)
all: ALLVAR = xxx
foo.x: FOOVAR = bar
rules.mk : MYVAR = foo
.INTERMEDIATE: foo.x rules.mk
