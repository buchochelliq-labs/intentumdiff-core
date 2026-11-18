
base_metals_fmd_reports.sun5 base_metals_fmd_reports CreateRealPositions        CreateMarginFunds deals_changed_since : BUILD_OBJ=$(shell if [ -f               "build_information.generate" ]; then echo "$(OBJ_DIR)/build_information.o"; else echo "no build information"; fi  )

deals_changed_since: ; @echo $(BUILD_OBJ)
