################################################################################
# ANGI desktop, built from its standalone Cargo workspace.
################################################################################
AGX_GUI_VERSION = 0.4.0
AGX_GUI_SITE = $(BR2_EXTERNAL_AGX_PATH)/../.cache/agx-package
AGX_GUI_SITE_METHOD = local
AGX_GUI_SUBDIR = crates/agx-gui
AGX_GUI_DEPENDENCIES = libxcrypt libinput libxkbcommon fontconfig libudev libdrm mesa3d host-pkgconf
AGX_GUI_CARGO_ENV = PKG_CONFIG_ALLOW_CROSS=1
AGX_GUI_CARGO_BUILD_OPTS = --no-default-features --features kms

define AGX_GUI_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/$(AGX_GUI_SUBDIR)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/agx-gui $(TARGET_DIR)/usr/bin/agx-gui
	$(INSTALL) -D -m 0644 $(@D)/$(AGX_GUI_SUBDIR)/ui/fonts/JetBrainsMonoNerdFont-Regular.ttf $(TARGET_DIR)/usr/share/fonts/JetBrainsMonoNerdFont-Regular.ttf
	ln -sf agx-gui $(TARGET_DIR)/usr/bin/open
endef

$(eval $(cargo-package))
