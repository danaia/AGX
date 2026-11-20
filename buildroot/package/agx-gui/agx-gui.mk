AGX_GUI_VERSION = 0.4.0
AGX_GUI_SITE = $(BR2_EXTERNAL_AGX_PATH)/../.cache/agx-gui-package
AGX_GUI_SITE_METHOD = local
AGX_GUI_SUBDIR = crates/agx-gui
AGX_GUI_DEPENDENCIES = fontconfig libxcrypt host-pkgconf
AGX_GUI_CARGO_ENV = PKG_CONFIG_ALLOW_CROSS=1 CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=true
AGX_GUI_CARGO_BUILD_OPTS = --no-default-features --features headless

define AGX_GUI_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(AGX_GUI_SRCDIR)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx-gui $(TARGET_DIR)/usr/bin/agx-gui
endef

$(eval $(cargo-package))
