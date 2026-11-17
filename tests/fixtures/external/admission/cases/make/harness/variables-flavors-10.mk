
.POSIX:
recur = $\
  one$\
  two$\
  three
simple := $\
  four$\
  five$\
  six

all: d$\
     e$\
     p; @:

.PHONY: dep
dep: ; @: $(info recur=/$(recur)/ simple=/$(simple)/)
