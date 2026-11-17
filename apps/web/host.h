#ifndef AGX_WEB_HOST_H
#define AGX_WEB_HOST_H
#include <wpe/webkit.h>
#include <wpe/wpe-platform.h>
#include <wpe/headless/wpe-headless.h>
#include <glib-unix.h>
#include <stdint.h>
#define WEB_MAX_WIDTH 4096u
#define WEB_MAX_HEIGHT 2160u
#define WEB_FRAME_BYTES ((size_t)WEB_MAX_WIDTH * WEB_MAX_HEIGHT * 4u)
#define WEB_MAX_PACKET 65536u

typedef struct {
    GMainLoop *loop;
    WebKitWebView *web;
    WPEView *view;
    WebKitOptionMenu *menu;
    WebKitScriptDialog *dialog;
    int control;
    guint8 *pixels;
    GByteArray *input;
    WPEBuffer *in_flight;
    WPEBuffer *queued;
    uint64_t sequence;
    gboolean failed;
    double pointer_x, pointer_y;
} WebHost;

gboolean web_storage_setup(WebHost *host, char **data_dir, char **cache_dir, char **shader_dir);
void web_dialogs_connect(WebHost *host);
void web_menu_response(WebHost *host, guint16 index);
void web_dialog_response(WebHost *host, gboolean accepted, const char *text);

WPEDisplay *angi_display_new(WebHost *host);
gboolean web_render(WebHost *host, WPEView *view, WPEBuffer *buffer, GError **error);
void web_ack(WebHost *host, uint64_t sequence);
void web_discard_buffers(WebHost *host);
gboolean web_send(WebHost *host, guint8 kind, const void *data, size_t size);
void web_error(WebHost *host, const char *message);
void web_text(WebHost *host, guint8 kind, const char *text);
gboolean web_read_commands(gint fd, GIOCondition condition, gpointer data);
#endif
