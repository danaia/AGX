/* ANGI Web: a small WPEPlatform host with sandboxed web content, launched only for an open app. */
#include "host.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static void text_property(WebKitWebView *web, GParamSpec *property, gpointer data)
{
    WebHost *host = data;
    const char *value = !strcmp(property->name, "title") ? webkit_web_view_get_title(web) : webkit_web_view_get_uri(web);
    if (!value) value = "";
    web_text(host, !strcmp(property->name, "title") ? 2 : 3, value);
}
static void progress(WebKitWebView *web, GParamSpec *property, gpointer data)
{
    (void)property;
    guint8 status[2] = {
        (webkit_web_view_is_loading(web) ? 1 : 0) | (webkit_web_view_can_go_back(web) ? 2 : 0) | (webkit_web_view_can_go_forward(web) ? 4 : 0),
        (guint8)(100 * webkit_web_view_get_estimated_load_progress(web))
    };
    web_send(data, 4, status, sizeof(status));
}
static void load_changed(WebKitWebView *web, WebKitLoadEvent event, gpointer data)
{
    if (event == WEBKIT_LOAD_STARTED) web_error(data, "");
    progress(web, NULL, data);
}
static gboolean load_failed(WebKitWebView *web, WebKitLoadEvent event, const char *uri, GError *error, gpointer data)
{
    (void)web; (void)event; (void)uri;
    if (g_error_matches(error, G_IO_ERROR, G_IO_ERROR_CANCELLED)) return TRUE;
    web_error(data, "Could not load this page. Check the address and connection, then reload.");
    return TRUE;
}
static void process_terminated(WebKitWebView *web, WebKitWebProcessTerminationReason reason, gpointer data)
{
    (void)web;
    g_printerr("agx-web: web process ended (%d)\n", reason);
    web_error(data, "Web content stopped unexpectedly. Reload to try again.");
}
static gboolean stop(gpointer data) { g_main_loop_quit(data); return G_SOURCE_REMOVE; }
static int descriptor(const char *text)
{
    char *end = NULL;
    long value = strtol(text, &end, 10);
    return !*text || *end || value < 3 || value > 65535 ? -1 : (int)value;
}
int main(int argc, char **argv)
{
    g_set_prgname("agx-web");
    g_set_application_name("ANGI Web");
    if ((argc != 5 && argc != 7) || strcmp(argv[1], "--control-fd") || strcmp(argv[3], "--frame-fd")
        || (argc == 7 && strcmp(argv[5], "--gpu-fd"))) {
        g_printerr("agx-web is an embedding host; open Web from ANGI Applications.\n");
        return 2;
    }
    if (!geteuid()) { g_printerr("agx-web: refusing to run web content as root\n"); return 1; }
    int control = descriptor(argv[2]), frame = descriptor(argv[4]);
    int gpu_fd = argc == 7 ? descriptor(argv[6]) : -1;
    if (argc == 7 && (gpu_fd < 0 || gpu_fd == control || gpu_fd == frame || fcntl(gpu_fd, F_SETFD, FD_CLOEXEC))) return 2;
    struct stat info;
    if (control < 0 || frame < 0 || control == frame || fstat(frame, &info) || (size_t)info.st_size != WEB_FRAME_BYTES) return 2;
    if (fcntl(control, F_SETFD, FD_CLOEXEC)) return 1;
    guint8 *pixels = mmap(NULL, WEB_FRAME_BYTES, PROT_READ | PROT_WRITE, MAP_SHARED, frame, 0);
    close(frame);
    if (pixels == MAP_FAILED) { perror("agx-web: frame mapping"); return 1; }
    signal(SIGPIPE, SIG_IGN);
    WebHost host = { .control = control, .pixels = pixels, .gpu_fd = gpu_fd, .use_gpu = gpu_fd >= 0 };
    host.loop = g_main_loop_new(NULL, FALSE);
    host.input = g_byte_array_new();
    g_autofree char *data_dir = NULL, *cache_dir = NULL, *shader_dir = NULL;
    if (!web_storage_setup(&host, &data_dir, &cache_dir, &shader_dir)) return 1;
    g_autoptr(WPEDisplay) display = angi_display_new(&host);
    g_autoptr(GError) error = NULL;
    if (!wpe_display_connect(display, &error)) {
        g_printerr("agx-web: platform initialization failed: %s\n", error->message);
        web_error(&host, "Web could not initialize its graphics device. Check the GPU driver and permissions.");
        return 1;
    }
    /* WebKit also passes this read-only library directory to its sandboxed
     * auxiliary launchers, whose bind mounts must resolve NVMe symlinks. */
    const char *library_dir = "/var/lib/agx-data/agx-web/current/usr/lib";
    if (g_file_test(library_dir, G_FILE_TEST_IS_DIR)) {
        const char *previous = g_getenv("LD_LIBRARY_PATH");
        /* WebKit uses library search paths as read-only sandbox mount hints.
         * Auxiliary GPU/DBus sandboxes also need the real targets of runtime
         * executable and Mesa symlinks, without exposing private user stores. */
        g_autofree char *public_paths = g_strconcat(library_dir,
            ":/var/lib/agx-data/agx-web/releases:/var/lib/agx-data/agx-gui/releases", NULL);
        g_autofree char *paths = previous && *previous ? g_strconcat(public_paths, ":", previous, NULL) : g_strdup(public_paths);
        g_setenv("LD_LIBRARY_PATH", paths, TRUE);
    }
    g_unix_signal_add(SIGTERM, stop, host.loop);
    g_unix_signal_add(SIGINT, stop, host.loop);
    g_autoptr(WebKitWebContext) context = webkit_web_context_new();
    webkit_web_context_set_cache_model(context, WEBKIT_CACHE_MODEL_DOCUMENT_BROWSER);
    /* Runtime libraries live on NVMe; only these public, read-only runtime trees are exposed. */
    const char *runtime[] = { "/var/lib/agx-data/agx-web/releases", "/var/lib/agx-data/agx-gui/releases", NULL };
    for (int i = 0; runtime[i]; ++i)
        if (g_file_test(runtime[i], G_FILE_TEST_IS_DIR)) webkit_web_context_add_path_to_sandbox(context, runtime[i], TRUE);
    /* Only shader cache files are writable in the content sandbox. Cookie and
     * page-data stores remain behind WebKit's network/storage processes. */
    webkit_web_context_add_path_to_sandbox(context, shader_dir, FALSE);
    g_autoptr(WebKitNetworkSession) session = webkit_network_session_new(data_dir, cache_dir);
    webkit_network_session_set_itp_enabled(session, TRUE);
    g_autofree char *cookies = g_build_filename(data_dir, "cookies.sqlite", NULL);
    webkit_cookie_manager_set_persistent_storage(webkit_network_session_get_cookie_manager(session),
        cookies, WEBKIT_COOKIE_PERSISTENT_STORAGE_SQLITE);
    host.web = g_object_new(WEBKIT_TYPE_WEB_VIEW, "display", display,
        "web-context", context, "network-session", session, NULL);
    WebKitColor background = { 1, 1, 1, 1 };
    webkit_web_view_set_background_color(host.web, &background);
    WebKitSettings *settings = webkit_web_view_get_settings(host.web);
    g_object_set(settings, "enable-developer-extras", FALSE,
        "allow-file-access-from-file-urls", FALSE, "allow-universal-access-from-file-urls", FALSE, NULL);
    host.view = webkit_web_view_get_wpe_view(host.web);
    web_dialogs_connect(&host);
    g_signal_connect(host.web, "notify::title", G_CALLBACK(text_property), &host);
    g_signal_connect(host.web, "notify::uri", G_CALLBACK(text_property), &host);
    g_signal_connect(host.web, "notify::estimated-load-progress", G_CALLBACK(progress), &host);
    g_signal_connect(host.web, "load-changed", G_CALLBACK(load_changed), &host);
    g_signal_connect(host.web, "load-failed", G_CALLBACK(load_failed), &host);
    g_signal_connect(host.web, "web-process-terminated", G_CALLBACK(process_terminated), &host);
    g_autoptr(WPEToplevel) toplevel = wpe_display_create_toplevel(display, 1);
    wpe_view_set_toplevel(host.view, toplevel);
    wpe_toplevel_resize(toplevel, 800, 600);
    wpe_view_set_visible(host.view, FALSE);
    g_unix_fd_add(control, G_IO_IN | G_IO_HUP | G_IO_ERR, web_read_commands, &host);
    web_send(&host, 6, "ANGIWEB1", 8);
    g_main_loop_run(host.loop);
    web_menu_response(&host, G_MAXUINT16);
    web_dialog_response(&host, FALSE, "");
    web_discard_buffers(&host);
    g_object_unref(host.web);
    g_byte_array_unref(host.input);
    g_main_loop_unref(host.loop);
    munmap(pixels, WEB_FRAME_BYTES);
    close(control);
    if (gpu_fd >= 0) close(gpu_fd);
    return host.failed ? 1 : 0;
}
