
foo: bar | baz
	@echo '$$^ = $^'
	@echo '$$| = $|'
	touch $@

foo: baz

.PHONY: baz

bar baz:
	touch $@
