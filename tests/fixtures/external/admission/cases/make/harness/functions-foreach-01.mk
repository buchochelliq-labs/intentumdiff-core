
.PHONY: all target
all: target

x := BAD

define mktarget
target: x := $(x)
target: ; @echo "$(x)"
endef

x := GLOBAL

$(foreach x,FOREACH,$(eval $(value mktarget)))
