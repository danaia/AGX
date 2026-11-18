################################################################################
# ANGI Web embedding host and offline start page.
################################################################################
AGX_WEB_VERSION = 0.4.0
AGX_WEB_SITE = $(BR2_EXTERNAL_AGX_PATH)/../apps/web
AGX_WEB_SITE_METHOD = local
AGX_WEB_DEPENDENCIES = agx-webkit dejavu

define AGX_WEB_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -std=gnu11 -DAGX_WEB_APPLIANCE=1 -Wall -Wextra \
		$(@D)/host.c $(@D)/tabs.c $(@D)/platform.c $(@D)/protocol.c $(@D)/dialogs.c $(@D)/downloads.c $(@D)/storage.c $(@D)/gpu.c \
		`$(PKG_CONFIG_HOST_BINARY) --cflags --libs wpe-webkit-2.0 libdrm` \
		$(TARGET_LDFLAGS) -o $(@D)/agx-web
endef

define AGX_WEB_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/agx-web $(TARGET_DIR)/usr/libexec/agx-web
	$(INSTALL) -D -m 0644 $(@D)/start.html $(TARGET_DIR)/usr/share/agx/web/start.html
endef

$(eval $(generic-package))
