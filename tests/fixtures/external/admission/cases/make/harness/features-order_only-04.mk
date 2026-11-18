
%.w : %.x | baz
	@echo '$$^ = $^'
	@echo '$$| = $|'
	touch $@

all: foo.w

.PHONY: baz
foo.x baz:
	touch $@
