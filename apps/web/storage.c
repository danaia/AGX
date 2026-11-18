/* Browser state belongs on NVMe, separate from the public runtime and SD home. */
#include "host.h"
#include <glib/gstdio.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

static gboolean private_directory(const char *path)
{
    if (g_mkdir_with_parents(path, 0700)) return FALSE;
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) return FALSE;
    struct stat info;
    gboolean valid = !fstat(fd, &info) && S_ISDIR(info.st_mode) && info.st_uid == geteuid();
    if (valid) valid = !fchmod(fd, 0700);
    close(fd);
    return valid;
}
gboolean web_storage_setup(WebHost *host, char **data_dir, char **cache_dir, char **shader_dir, char **store_dir)
{
    struct stat volume, root, owner;
#ifdef AGX_WEB_APPLIANCE
    if (stat("/var/lib/agx-data", &volume)) {
        web_error(host, "Web needs the NVMe data volume. Mount it and sign in again.");
        return FALSE;
    }
    gboolean appliance = TRUE;
#else
    gboolean appliance = !stat("/var/lib/agx-data", &volume);
#endif
    if (appliance) {
        if (stat("/", &root) || volume.st_dev == root.st_dev) {
            web_error(host, "Web needs the NVMe data volume. Mount it and sign in again.");
            return FALSE;
        }
        g_autofree char *user = g_strdup_printf("/var/lib/agx-data/agx-web-users/%lu", (unsigned long)geteuid());
        if (lstat(user, &owner) || !S_ISDIR(owner.st_mode) || owner.st_uid != geteuid()
            || owner.st_dev != volume.st_dev || (owner.st_mode & 0077)) {
            web_error(host, "Web storage is not ready for this account. Sign out and back in.");
            return FALSE;
        }
        *store_dir = g_strdup(user);
        *data_dir = g_build_filename(user, "data", NULL);
        *cache_dir = g_build_filename(user, "cache", NULL);
    } else {
        *data_dir = g_build_filename(g_get_user_data_dir(), "agx", "web", NULL);
        *cache_dir = g_build_filename(g_get_user_cache_dir(), "agx", "web", NULL);
    }
    if (!private_directory(*data_dir) || !private_directory(*cache_dir)) {
        web_error(host, "Web could not create its private storage directories.");
        return FALSE;
    }
    /* Set cache locations before creating the graphics display or web context. */
    if (appliance) g_setenv("XDG_CACHE_HOME", *cache_dir, TRUE);
    *shader_dir = g_build_filename(*cache_dir, "shaders", NULL);
    g_autofree char *font_cache = g_build_filename(*cache_dir, "fontconfig", NULL);
    if (!private_directory(*shader_dir) || !private_directory(font_cache)) {
        web_error(host, "Web could not create its graphics caches.");
        return FALSE;
    }
    g_setenv("MESA_SHADER_CACHE_DIR", *shader_dir, TRUE);
    if (!g_getenv("MESA_SHADER_CACHE_MAX_SIZE")) g_setenv("MESA_SHADER_CACHE_MAX_SIZE", "64M", TRUE);
    return TRUE;
}
