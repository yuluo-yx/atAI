_run:
	@$(MAKE) --no-print-directory --warn-undefined-variables \
		-f tools/make/common.mk \
		-f tools/make/rust.mk \
		$(if $(MAKECMDGOALS),$(MAKECMDGOALS),help)

.PHONY: _run

$(if $(MAKECMDGOALS),$(MAKECMDGOALS): %: _run)
