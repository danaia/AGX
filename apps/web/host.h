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
#define WEB_MAX_TABS 12u
#define WEB_HOME "https://www.google.com/"

typedef struct {
    GMainLoop *loop;
    WebKitWebView *web;
    WPEDisplay *display;
    WebKitWebContext *context;
    WebKitNetworkSession *session;
    GPtrArray *tabs;
    guint active;
    guint width, height;
    gboolean visible, focused, closing_tab, tabs_enabled;
    WPEView *view;
    WebKitOptionMenu *menu;
    GPtrArray *menu_actions;
    WebKitScriptDialog *dialog;
    int control;
    int gpu_fd;
    gboolean use_gpu;
    guint8 *pixels;
    GByteArray *input;
    WPEView *flight_view;
    gboolean flight_replay;
    WPEBuffer *in_flight;
    WPEBuffer *queued;
    uint64_t sequence, tab_frame_floor;
    gboolean failed;
    char *downloads;
    double pointer_x, pointer_y;
} WebHost;

gboolean web_storage_setup(WebHost *host, char **data_dir, char **cache_dir, char **shader_dir, char **store_dir);
void web_dialogs_connect(WebHost *host, WebKitWebView *web);
void web_downloads_connect(WebHost *host, const char *store);
void web_menu_response(WebHost *host, guint16 index);
void web_dialog_response(WebHost *host, gboolean accepted, const char *text);

WPEDisplay *angi_display_new(WebHost *host);
gboolean web_render(WebHost *host, WPEView *view, WPEBuffer *buffer, GError **error);
gboolean web_gpu_publish(WebHost *host, WPEBuffer *buffer, int width, int height);
void web_gpu_fallback(WebHost *host, uint64_t sequence);
void web_ack(WebHost *host, uint64_t sequence);
void web_discard_buffers(WebHost *host);
void web_discard_queued(WebHost *host);
void web_restore_view(WebHost *host);
void web_retire_view(WPEView *view);
void web_tab_command(WebHost *host, guint action, guint index);
void web_tab_closed(WebHost *host, WebKitWebView *web);
void web_tab_state(WebHost *host);
void web_tab_error(WebHost *host, WebKitWebView *web, const char *message);
gboolean web_send(WebHost *host, guint8 kind, const void *data, size_t size);
void web_error(WebHost *host, const char *message);
void web_text(WebHost *host, guint8 kind, const char *text);
gboolean web_read_commands(gint fd, GIOCondition condition, gpointer data);
#endif
