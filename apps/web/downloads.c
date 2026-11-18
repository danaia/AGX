/* Downloads save into the signed-in user's ~/Downloads; the GUI only receives short status notices. */
#include "host.h"
#include <glib/gstdio.h>
#include <errno.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/statvfs.h>

#define WEB_DOWNLOAD_RESERVE (64ull * 1024 * 1024)

/* Keep the engine's suggested name from escaping the folder or hiding the file. */
static char *safe_name(const char *suggested)
{
    g_autofree char *base = g_path_get_basename(suggested && *suggested ? suggested : "download");
    GString *name = g_string_new(NULL);
    for (const char *p = base; *p; p = g_utf8_next_char(p)) {
        gunichar c = g_utf8_get_char_validated(p, -1);
        if (c == (gunichar)-1 || c == (gunichar)-2) break;
        if (c < 0x20 || c == 0x7f || c == '/' || c == '\\') c = '_';
        if (name->len + 4 > 200) break;
        g_string_append_unichar(name, c);
    }
    while (name->len && name->str[0] == '.') g_string_erase(name, 0, 1);
    if (!name->len || !g_strcmp0(name->str, "..")) g_string_assign(name, "download");
    return g_string_free(name, FALSE);
}
/* ~/Downloads is the user's folder. On the appliance the SD root is small, so when the user has no folder
 * of that name it becomes a link to a private folder in their NVMe store; an existing folder or link is honoured. */
static char *resolve_directory(const char *store)
{
    const char *configured = g_getenv("AGX_WEB_DOWNLOADS");
    if (configured && g_path_is_absolute(configured)) return g_strdup(configured);
    g_autofree char *home_link = g_build_filename(g_get_home_dir(), "Downloads", NULL);
    if (store) {
        g_autofree char *target = g_build_filename(store, "downloads", NULL);
        if (g_mkdir(target, 0700) && !g_file_test(target, G_FILE_TEST_IS_DIR)) return NULL;
        GStatBuf info;
        if (g_lstat(home_link, &info) && symlink(target, home_link) && errno != EEXIST) return g_steal_pointer(&target);
    } else if (g_mkdir(home_link, 0700) && !g_file_test(home_link, G_FILE_TEST_IS_DIR)) return NULL;
    return g_steal_pointer(&home_link);
}
/* name.ext, name (1).ext, name (2).ext ... without replacing anything already saved. */
static char *unique_path(const char *directory, const char *name)
{
    g_autofree char *stem = g_strdup(name);
    const char *extension = "";
    char *dot = strrchr(stem, '.');
    if (dot && dot != stem) { extension = name + (dot - stem); *dot = 0; }
    for (guint i = 0; i < 1000; ++i) {
        g_autofree char *candidate = i ? g_strdup_printf("%s (%u)%s", stem, i, extension) : g_strdup(name);
        char *path = g_build_filename(directory, candidate, NULL);
        if (!g_file_test(path, G_FILE_TEST_EXISTS)) return path;
        g_free(path);
    }
    return NULL;
}
static void notice(WebHost *host, const char *format, const char *name)
{
    g_autofree char *text = g_strdup_printf(format, name);
    web_text(host, 13, text);
}
static void download_finished(WebKitDownload *download, WebHost *host)
{
    g_autofree char *name = g_path_get_basename(webkit_download_get_destination(download) ?: "file");
    notice(host, "Saved %s to Downloads", name);
}
static void download_failed(WebKitDownload *download, GError *error, WebHost *host)
{
    g_autofree char *name = g_path_get_basename(webkit_download_get_destination(download) ?: "file");
    if (g_error_matches(error, WEBKIT_DOWNLOAD_ERROR, WEBKIT_DOWNLOAD_ERROR_CANCELLED_BY_USER))
        notice(host, "Download of %s cancelled", name);
    else notice(host, "Could not download %s. Check the connection and free space.", name);
}
static gboolean decide_destination(WebKitDownload *download, const char *suggested, WebHost *host)
{
    const char *directory = host->downloads;
    g_autofree char *name = safe_name(suggested);
    g_autofree char *path = directory ? unique_path(directory, name) : NULL;
    if (!path) {
        web_text(host, 13, "Downloads folder is unavailable. Check your home directory.");
        webkit_download_cancel(download);
        return TRUE;
    }
    struct statvfs space;
    guint64 length = webkit_uri_response_get_content_length(webkit_download_get_response(download));
    if (length && !statvfs(directory, &space)
        && length + WEB_DOWNLOAD_RESERVE > (guint64)space.f_bavail * space.f_frsize) {
        notice(host, "Not enough free space to download %s", name);
        webkit_download_cancel(download);
        return TRUE;
    }
    webkit_download_set_allow_overwrite(download, FALSE);
    webkit_download_set_destination(download, path);
    g_signal_connect(download, "finished", G_CALLBACK(download_finished), host);
    g_signal_connect(download, "failed", G_CALLBACK(download_failed), host);
    notice(host, "Downloading %s…", name);
    return TRUE;
}
static void download_started(WebKitNetworkSession *session, WebKitDownload *download, WebHost *host)
{
    (void)session;
    g_signal_connect(download, "decide-destination", G_CALLBACK(decide_destination), host);
}
void web_downloads_connect(WebHost *host, const char *store)
{
    /* Resolve at startup so the folder exists before the first download. */
    host->downloads = resolve_directory(store);
    g_signal_connect(host->session, "download-started", G_CALLBACK(download_started), host);
}
