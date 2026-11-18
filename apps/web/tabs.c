/* Independent WebKit views share one sandboxed context and network session. */
#include "host.h"
#include <string.h>

void web_tab_state(WebHost *host)
{
    if (!host->tabs_enabled || !host->tabs->len) return;
    guint8 active = host->active;
    g_autoptr(GByteArray) packet = g_byte_array_new();
    guint64 frame_floor = GUINT64_TO_BE(host->tab_frame_floor);
    g_byte_array_append(packet, (const guint8 *)&frame_floor, sizeof(frame_floor));
    g_byte_array_append(packet, &active, 1);
    for (guint i = 0; i < host->tabs->len; i++) {
        WebKitWebView *web = g_ptr_array_index(host->tabs, i);
        const char *title = webkit_web_view_get_title(web);
        if (!title || !*title) title = "New tab";
        size_t size = MIN(strlen(title), 240u);
        while (size && ((guint8)title[size] & 0xc0) == 0x80) --size;
        guint8 length = size;
        g_byte_array_append(packet, &length, 1);
        g_byte_array_append(packet, (const guint8 *)title, size);
    }
    web_send(host, 12, packet->data, packet->len);
}

static void title_changed(WebKitWebView *web, GParamSpec *property, WebHost *host)
{
    (void)property;
    web_tab_state(host);
    if (web == host->web) web_text(host, 2, webkit_web_view_get_title(web) ?: "");
}
static void uri_changed(WebKitWebView *web, GParamSpec *property, WebHost *host)
{
    (void)property;
    if (web == host->web) web_text(host, 3, webkit_web_view_get_uri(web) ?: WEB_HOME);
}
static void progress(WebKitWebView *web, GParamSpec *property, WebHost *host)
{
    (void)property;
    if (web != host->web) return;
    guint8 status[2] = {
        (webkit_web_view_is_loading(web) ? 1 : 0) |
        (webkit_web_view_can_go_back(web) ? 2 : 0) |
        (webkit_web_view_can_go_forward(web) ? 4 : 0),
        (guint8)(100 * webkit_web_view_get_estimated_load_progress(web))
    };
    web_send(host, 4, status, sizeof(status));
}
void web_tab_error(WebHost *host, WebKitWebView *web, const char *message)
{
    g_object_set_data_full(G_OBJECT(web), "angi-error", g_strdup(message), g_free);
    if (web == host->web) web_error(host, message);
}
static void load_changed(WebKitWebView *web, WebKitLoadEvent event, WebHost *host)
{
    if (event == WEBKIT_LOAD_STARTED) web_tab_error(host, web, "");
    progress(web, NULL, host);
}
static gboolean load_failed(WebKitWebView *web, WebKitLoadEvent event, const char *uri, GError *error, WebHost *host)
{
    (void)event; (void)uri;
    if (!g_error_matches(error, G_IO_ERROR, G_IO_ERROR_CANCELLED))
        web_tab_error(host, web, "Could not load this page. Check the address and connection, then reload.");
    return TRUE;
}
static void process_terminated(WebKitWebView *web, WebKitWebProcessTerminationReason reason, WebHost *host)
{
    g_printerr("agx-web: web process ended (%d)\n", reason);
    web_tab_error(host, web, "Web content stopped unexpectedly. Reload to try again.");
}
static void select_tab(WebHost *host, guint index)
{
    web_menu_response(host, G_MAXUINT16);
    web_dialog_response(host, FALSE, "");
    web_discard_queued(host);
    WPEView *previous = host->view;
    host->tab_frame_floor = host->sequence + 1;
    host->active = index;
    host->web = g_ptr_array_index(host->tabs, index);
    host->view = webkit_web_view_get_wpe_view(host->web);
    if (previous && previous != host->view) {
        wpe_view_focus_out(previous);
        wpe_view_set_visible(previous, FALSE);
        wpe_view_unmap(previous);
    }
    web_tab_state(host);
    web_text(host, 2, webkit_web_view_get_title(host->web) ?: "");
    uri_changed(host->web, NULL, host);
    progress(host->web, NULL, host);
    web_error(host, g_object_get_data(G_OBJECT(host->web), "angi-error") ?: "");
    wpe_toplevel_resize(wpe_view_get_toplevel(host->view), host->width, host->height);
    wpe_view_set_visible(host->view, host->visible);
    if (host->visible) wpe_view_map(host->view);
    if (host->focused) wpe_view_focus_in(host->view);
    web_restore_view(host);
}
void web_tab_command(WebHost *host, guint action, guint index)
{
    if (host->dialog || host->closing_tab) return;
    if (action == 0 && host->tabs->len < WEB_MAX_TABS) {
        gboolean first = host->tabs->len == 0;
        WebKitWebView *web = g_object_new(WEBKIT_TYPE_WEB_VIEW, "display", host->display,
            "web-context", host->context, "network-session", host->session, NULL);
        WebKitColor background = { 1, 1, 1, 1 };
        webkit_web_view_set_background_color(web, &background);
        g_object_set(webkit_web_view_get_settings(web), "enable-developer-extras", FALSE,
            "allow-file-access-from-file-urls", FALSE, "allow-universal-access-from-file-urls", FALSE, NULL);
        WPEView *view = webkit_web_view_get_wpe_view(web);
        g_autoptr(WPEToplevel) toplevel = wpe_display_create_toplevel(host->display, 1);
        wpe_view_set_toplevel(view, toplevel);
        wpe_view_set_visible(view, FALSE);
        web_dialogs_connect(host, web);
        g_signal_connect(web, "notify::title", G_CALLBACK(title_changed), host);
        g_signal_connect(web, "notify::uri", G_CALLBACK(uri_changed), host);
        g_signal_connect(web, "notify::estimated-load-progress", G_CALLBACK(progress), host);
        g_signal_connect(web, "load-changed", G_CALLBACK(load_changed), host);
        g_signal_connect(web, "load-failed", G_CALLBACK(load_failed), host);
        g_signal_connect(web, "web-process-terminated", G_CALLBACK(process_terminated), host);
        g_ptr_array_add(host->tabs, web);
        if (first) {
            // GUI sends the initial address after the ready handshake.
            host->tab_frame_floor = 1;
            host->web = web;
            host->view = view;
            wpe_toplevel_resize(toplevel, host->width, host->height);
            web_tab_state(host);
        } else {
            select_tab(host, host->tabs->len - 1);
            webkit_web_view_load_uri(web, WEB_HOME);
        }
    } else if (action == 1 && index < host->tabs->len && index != host->active) {
        select_tab(host, index);
    } else if (action == 2 && index == host->active) {
        host->closing_tab = TRUE;
        webkit_web_view_try_close(host->web);
    }
}
void web_tab_closed(WebHost *host, WebKitWebView *web)
{
    if (web != host->web) return;
    if (!host->closing_tab || host->tabs->len == 1) {
        web_send(host, 11, NULL, 0);
        return;
    }
    host->closing_tab = FALSE;
    guint next = MIN(host->active, host->tabs->len - 2);
    g_autoptr(WebKitWebView) removed = g_ptr_array_steal_index(host->tabs, host->active);
    g_signal_handlers_disconnect_by_data(removed, host);
    select_tab(host, next);
    web_retire_view(webkit_web_view_get_wpe_view(removed));
}
