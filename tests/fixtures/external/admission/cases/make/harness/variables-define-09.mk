
define FOO
$(V1)echo hello
$(V2)echo world
endef
all: ; @$(FOO)
