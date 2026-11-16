################################################################################
# AGX local Rust workspace; scripts/prepare-rust-package.sh vendors locked crates.
################################################################################
AGX_VERSION = 0.4.0
AGX_SITE = $(BR2_EXTERNAL_AGX_PATH)/../.cache/agx-package
AGX_SITE_METHOD = local
AGX_DEPENDENCIES += libxcrypt
AGX_CARGO_BUILD_OPTS = --workspace

define AGX_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx $(TARGET_DIR)/usr/bin/agx
	$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx-netd $(TARGET_DIR)/usr/sbin/agx-netd
	$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx-dhcp $(TARGET_DIR)/usr/sbin/agx-dhcp
endef

$(eval $(cargo-package))
