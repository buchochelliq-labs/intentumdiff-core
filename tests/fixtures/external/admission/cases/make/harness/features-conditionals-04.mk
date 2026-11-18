
ifneq ($(FOO),yes)
target:
else
BAR = bar
target:
endif
	@echo one
