
recur =
recur += foo
simple :=
simple += bar
recur_empty = foo
recur_empty +=
simple_empty := bar
simple_empty +=
empty_recur =
empty_recur +=
empty_simple :=
empty_simple +=

all: ; @: $(info recur=/$(recur)/ simple=/$(simple)/ recure=/$(recur_empty)/ simplee=/$(simple_empty)/ erecur=/$(empty_recur)/ esimple=/$(empty_simple)/)
