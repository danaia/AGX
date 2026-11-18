/* Private length-prefixed control channel; rendering buffers never enter this pipe. */
#include "host.h"
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <unistd.h>
#include <glib/gstdio.h>
#include <sys/socket.h>
#include <sys/stat.h>

static gboolean write_all(WebHost *host, const void *data, size_t size)
{
    const guint8 *bytes = data;
    while (size) {
        ssize_t n = write(host->control, bytes, size);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { host->failed = TRUE; g_main_loop_quit(host->loop); return FALSE; }
        bytes += n; size -= (size_t)n;
    }
    return TRUE;
}
gboolean web_send(WebHost *host, guint8 kind, const void *data, size_t size)
{
    if (host->failed || size >= WEB_MAX_PACKET) return FALSE;
    guint32 length = GUINT32_TO_BE((guint32)size + 1);
    guint8 header[5]; memcpy(header, &length, 4); header[4] = kind;
    return write_all(host, header, 5) && (!size || write_all(host, data, size));
}
void web_text(WebHost *host, guint8 kind, const char *text)
{
    size_t size = MIN(strlen(text), 8192u);
    while (size && ((guint8)text[size] & 0xc0) == 0x80) --size;
    web_send(host, kind, text, size);
}
void web_error(WebHost *host, const char *message)
{
    web_text(host, 5, message);
}
static guint16 u16(const guint8 *p) { guint16 n; memcpy(&n, p, 2); return GUINT16_FROM_BE(n); }
static guint32 u32(const guint8 *p) { guint32 n; memcpy(&n, p, 4); return GUINT32_FROM_BE(n); }
static guint64 u64(const guint8 *p) { guint64 n; memcpy(&n, p, 8); return GUINT64_FROM_BE(n); }
static float f32(const guint8 *p) { guint32 n = u32(p); float f; memcpy(&f, &n, 4); return f; }
static guint32 timestamp(void) { return (guint32)(g_get_monotonic_time() / 1000); }

#define WEB_MAX_LOCAL_HTML (16u * 1024u * 1024u)

/* Open a local .html/.htm page (from `open FILE` in a terminal). Returns FALSE when `uri` is not one, so the
 * caller loads it normally. The image's shared MIME database classifies .html as XHTML, and WebKit would then
 * parse ordinary HTML5 as XML ("StartTag: invalid element name"), so the page is loaded as bytes labelled
 * text/html. Its own address stays the base, so relative images, styles and links still resolve; the engine
 * still refuses file-to-file access, so the page cannot read other local files. */
static gboolean load_local_html(WebHost *host, const char *uri)
{
    g_autofree char *path = g_filename_from_uri(uri, NULL, NULL);
    if (!path) return FALSE;
    g_autofree char *lower = g_ascii_strdown(path, -1);
    if (!g_str_has_suffix(lower, ".html") && !g_str_has_suffix(lower, ".htm")) return FALSE;
    GStatBuf info;
    if (g_stat(path, &info) || !S_ISREG(info.st_mode) || info.st_size > (goffset)WEB_MAX_LOCAL_HTML) {
        web_error(host, "That page is not a regular file or is larger than 16 MB.");
        return TRUE;
    }
    g_autofree char *html = NULL;
    gsize length = 0;
    if (!g_file_get_contents(path, &html, &length, NULL)) {
        web_error(host, "That page cannot be read.");
        return TRUE;
    }
    g_autoptr(GBytes) bytes = g_bytes_new_take(g_steal_pointer(&html), length);
    /* No charset: WebKit then honours a byte-order mark or <meta charset>, as for any local file. */
    webkit_web_view_load_bytes(host->web, bytes, "text/html", NULL, uri);
    return TRUE;
}

static gboolean command(WebHost *host, const guint8 *p, guint32 size)
{
    guint8 kind = *p++; --size;
    g_autoptr(WPEEvent) event = NULL;
    switch (kind) {
    case 1:
        if (size != 4 || !u16(p) || !u16(p + 2) || u16(p) > WEB_MAX_WIDTH || u16(p + 2) > WEB_MAX_HEIGHT) return FALSE;
        wpe_toplevel_resize(wpe_view_get_toplevel(host->view), u16(p), u16(p + 2));
        return TRUE;
    case 2: {
        if (!size || size > 8192 || memchr(p, 0, size) || !g_utf8_validate((const char *)p, size, NULL)) return FALSE;
        g_autofree char *uri = g_strndup((const char *)p, size);
        if (!g_str_has_prefix(uri, "https://") && !g_str_has_prefix(uri, "http://") && !g_str_has_prefix(uri, "file://")) return FALSE;
        web_error(host, "");
        if (!strcmp(uri, "file:///usr/share/agx/web/start.html")) {
            /* Shared MIME glob databases can classify .html as XHTML without
             * sniffing. Our bundled HTML5 document has a known content type. */
            g_autofree char *html = NULL;
            gsize length = 0;
            if (!g_file_get_contents("/usr/share/agx/web/start.html", &html, &length, NULL)) {
                web_error(host, "The Web start page is unavailable.");
                return TRUE;
            }
            g_autoptr(GBytes) bytes = g_bytes_new_take(g_steal_pointer(&html), length);
            webkit_web_view_load_bytes(host->web, bytes, "text/html", "UTF-8", uri);
        } else if (!load_local_html(host, uri)) webkit_web_view_load_uri(host->web, uri);
        return TRUE;
    }
    case 3: if (size) return FALSE; webkit_web_view_go_back(host->web); return TRUE;
    case 4: if (size) return FALSE; webkit_web_view_go_forward(host->web); return TRUE;
    case 5: if (size) return FALSE; web_error(host, ""); webkit_web_view_reload(host->web); return TRUE;
    case 6:
        if (size != 1) return FALSE;
        wpe_view_set_visible(host->view, p[0] != 0);
        if (p[0]) wpe_view_map(host->view); else wpe_view_unmap(host->view);
        return TRUE;
    case 7:
        if (size != 1) return FALSE;
        if (p[0]) wpe_view_focus_in(host->view); else wpe_view_focus_out(host->view);
        return TRUE;
    case 8: {
        if (size != 10 || p[0] > 2 || p[1] > 3) return FALSE;
        double x = u16(p + 2), y = u16(p + 4);
        WPEModifiers mods = (WPEModifiers)u32(p + 6);
        if (!p[0])
            event = wpe_event_pointer_move_new(WPE_EVENT_POINTER_MOVE, host->view, WPE_INPUT_SOURCE_MOUSE,
                timestamp(), mods, x, y, x - host->pointer_x, y - host->pointer_y);
        else {
            guint count = p[0] == 1 ? wpe_view_compute_press_count(host->view, x, y, p[1], timestamp()) : 0;
            event = wpe_event_pointer_button_new(p[0] == 1 ? WPE_EVENT_POINTER_DOWN : WPE_EVENT_POINTER_UP,
                host->view, WPE_INPUT_SOURCE_MOUSE, timestamp(), mods, p[1], x, y, count);
        }
        host->pointer_x = x; host->pointer_y = y;
        break;
    }
    case 9:
        if (size != 9) return FALSE;
        event = wpe_event_keyboard_new(p[0] ? WPE_EVENT_KEYBOARD_KEY_DOWN : WPE_EVENT_KEYBOARD_KEY_UP,
            host->view, WPE_INPUT_SOURCE_KEYBOARD, timestamp(), (WPEModifiers)u32(p + 5), 0, u32(p + 1));
        break;
    case 10:
        if (size != 16) return FALSE;
        /* WPE 2.54 multiplies precise mouse deltas by 2.5 internally.
         * The private protocol uses Slint logical pixels, positive up/left. */
        event = wpe_event_scroll_new(host->view, WPE_INPUT_SOURCE_MOUSE, timestamp(),
            (WPEModifiers)u32(p + 12), f32(p) / 2.5, f32(p + 4) / 2.5, TRUE, FALSE, u16(p + 8), u16(p + 10));
        break;
    case 11: if (size != 8) return FALSE; web_ack(host, u64(p)); return TRUE;
    case 12: if (size) return FALSE; g_main_loop_quit(host->loop); return TRUE;
    case 13: if (size != 2) return FALSE; web_menu_response(host, u16(p)); return TRUE;
    case 14: {
        if (!size || size > 8193 || memchr(p + 1, 0, size - 1) || !g_utf8_validate((const char *)p + 1, size - 1, NULL)) return FALSE;
        g_autofree char *text = g_strndup((const char *)p + 1, size - 1);
        web_dialog_response(host, p[0] != 0, text); return TRUE;
    }
    case 16: if (size != 8) return FALSE; web_gpu_fallback(host, u64(p)); return TRUE;
    case 15: if (size) return FALSE; webkit_web_view_try_close(host->web); return TRUE;
    default: return FALSE;
    }
    if (event) wpe_view_event(host->view, event);
    return TRUE;
}
gboolean web_read_commands(gint fd, GIOCondition condition, gpointer data)
{
    WebHost *host = data;
    if (condition & (G_IO_HUP | G_IO_ERR)) { g_main_loop_quit(host->loop); return G_SOURCE_REMOVE; }
    guint8 buffer[4096];
    ssize_t n;
    while ((n = recv(fd, buffer, sizeof(buffer), MSG_DONTWAIT)) > 0) {
        if (host->input->len + (gsize)n > WEB_MAX_PACKET + 4) goto invalid;
        g_byte_array_append(host->input, buffer, n);
        while (host->input->len >= 4) {
            guint32 size = u32(host->input->data);
            if (!size || size > WEB_MAX_PACKET) goto invalid;
            if (host->input->len < size + 4) break;
            if (!command(host, host->input->data + 4, size)) goto invalid;
            g_byte_array_remove_range(host->input, 0, size + 4);
        }
    }
    if (!n || (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR)) goto invalid;
    return G_SOURCE_CONTINUE;
invalid:
    g_main_loop_quit(host->loop);
    return G_SOURCE_REMOVE;
}
