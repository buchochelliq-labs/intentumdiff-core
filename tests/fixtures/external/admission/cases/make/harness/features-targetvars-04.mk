
.PHONY: a

BLAH := foo
COMMAND = echo $(BLAH)

a: ; @$(COMMAND)

a: BLAH := bar
a: COMMAND += snafu $(BLAH)
