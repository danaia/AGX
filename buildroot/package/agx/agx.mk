################################################################################
# AGX local Rust workspace; scripts/prepare-rust-package.sh vendors locked crates.
################################################################################
AGX_VERSION = 0.2.0
AGX_SITE = $(BR2_EXTERNAL_AGX_PATH)/../.cache/agx-package
AGX_SITE_METHOD = local
AGX_CARGO_BUILD_OPTS = --package agx-cli --bin agx

define AGX_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx $(TARGET_DIR)/usr/bin/agx
endef

$(eval $(cargo-package))
