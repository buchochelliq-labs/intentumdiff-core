
foo:| baz
	@echo '$$^ = $^'
	@echo '$$| = $|'
	touch $@

.PHONY: baz

baz:
	touch $@
