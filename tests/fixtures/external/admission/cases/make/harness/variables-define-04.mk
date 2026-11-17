
define outer
 define inner
  A = B
 endef
endef

$(eval $(outer))

outer: ; @echo $(inner)
