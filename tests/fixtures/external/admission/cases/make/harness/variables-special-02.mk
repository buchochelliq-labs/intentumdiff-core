
define foo
: foo-one\
foo-two
: foo-three
	: foo-four
endef

orig: ; : orig-one
	: orig-two \
orig-three \
	orig-four \
		orig-five \\
	: orig-six
	$(foo)

.RECIPEPREFIX = >
test: ; : test-one
>: test-two \
test-three \
>test-four \
>	test-five \\
>: test-six
>$(foo)

.RECIPEPREFIX =
reset: ; : reset-one
	: reset-two \
reset-three \
	reset-four \
		reset-five \\
	: reset-six
	$(foo)
