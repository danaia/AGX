/* WPEPlatform embedding: GPU rendering offscreen, bounded shared-memory delivery.
 * The software ANGI/VNC desktop needs CPU pixels; no whole-desktop capture is used.
 */
#include "host.h"
#include <drm_fourcc.h>
#include <string.h>

typedef struct { WPEDisplay parent; WPEDisplay *gpu; } AngiDisplay;
typedef struct { WPEDisplayClass parent; } AngiDisplayClass;
typedef struct { WPEView parent; } AngiView;
typedef struct { WPEViewClass parent; } AngiViewClass;
G_DEFINE_TYPE(AngiDisplay, angi_display, WPE_TYPE_DISPLAY)
G_DEFINE_TYPE(AngiView, angi_view, WPE_TYPE_VIEW)

static gboolean view_render(WPEView *view, WPEBuffer *buffer,
    const WPERectangle *damage, guint n_damage, GError **error)
{
    (void)damage; (void)n_damage;
    WebHost *host = g_object_get_data(G_OBJECT(wpe_view_get_display(view)), "angi-host");
    gboolean rendered = web_render(host, view, buffer, error);
    if (!rendered) web_error(host, error && *error ? (*error)->message : "Web frame delivery failed");
    return rendered;
}
static void view_cursor(WPEView *view, const char *name)
{
    WebHost *host = g_object_get_data(G_OBJECT(wpe_view_get_display(view)), "angi-host");
    guint8 cursor = !g_strcmp0(name, "text") || !g_strcmp0(name, "vertical-text") ? 7
        : !g_strcmp0(name, "pointer") || !g_strcmp0(name, "hand2") ? 6 : 0;
    web_send(host, 7, &cursor, 1);
}
static void angi_view_class_init(AngiViewClass *klass)
{
    WPE_VIEW_CLASS(klass)->render_buffer = view_render;
    WPE_VIEW_CLASS(klass)->set_cursor_from_name = view_cursor;
}
static void angi_view_init(AngiView *view) { (void)view; }
static gboolean display_connect(WPEDisplay *display, GError **error)
{
    AngiDisplay *self = (AngiDisplay *)display;
    self->gpu = wpe_display_headless_new();
    wpe_display_set_available_input_devices(display, WPE_AVAILABLE_INPUT_DEVICE_MOUSE | WPE_AVAILABLE_INPUT_DEVICE_KEYBOARD);
    return wpe_display_connect(self->gpu, error);
}
static WPEView *display_create_view(WPEDisplay *display)
{
    return WPE_VIEW(g_object_new(angi_view_get_type(), "display", display, NULL));
}
static WPEToplevel *display_create_toplevel(WPEDisplay *display, guint max_views)
{
    (void)max_views;
    return WPE_TOPLEVEL(g_object_new(WPE_TYPE_TOPLEVEL_HEADLESS, "display", display, NULL));
}
static gpointer display_egl(WPEDisplay *display, GError **error)
{
    return wpe_display_get_egl_display(((AngiDisplay *)display)->gpu, error);
}
static WPEDRMDevice *display_drm(WPEDisplay *display)
{
    return wpe_display_get_drm_device(((AngiDisplay *)display)->gpu);
}
static WPEBufferFormats *display_formats(WPEDisplay *display)
{
    WPEDRMDevice *device = display_drm(display);
    WPEBufferFormatsBuilder *builder = wpe_buffer_formats_builder_new(device);
    wpe_buffer_formats_builder_append_group(builder, device, WPE_BUFFER_FORMAT_USAGE_MAPPING);
    wpe_buffer_formats_builder_append_format(builder, DRM_FORMAT_ARGB8888, DRM_FORMAT_MOD_LINEAR);
    wpe_buffer_formats_builder_append_format(builder, DRM_FORMAT_XRGB8888, DRM_FORMAT_MOD_LINEAR);
    WPEBufferFormats *formats = wpe_buffer_formats_builder_end(builder);
    return formats;
}
static void display_dispose(GObject *object)
{
    g_clear_object(&((AngiDisplay *)object)->gpu);
    G_OBJECT_CLASS(angi_display_parent_class)->dispose(object);
}
static void angi_display_class_init(AngiDisplayClass *klass)
{
    G_OBJECT_CLASS(klass)->dispose = display_dispose;
    WPEDisplayClass *display = WPE_DISPLAY_CLASS(klass);
    display->connect = display_connect;
    display->create_view = display_create_view;
    display->create_toplevel = display_create_toplevel;
    display->get_egl_display = display_egl;
    display->get_drm_device = display_drm;
    display->get_preferred_buffer_formats = display_formats;
}
static void angi_display_init(AngiDisplay *display) { (void)display; }
WPEDisplay *angi_display_new(WebHost *host)
{
    WPEDisplay *display = WPE_DISPLAY(g_object_new(angi_display_get_type(), NULL));
    g_object_set_data(G_OBJECT(display), "angi-host", host);
    return display;
}

static void release_buffer(WebHost *host, WPEBuffer **buffer)
{
    if (!*buffer) return;
    wpe_view_buffer_rendered(host->view, *buffer);
    wpe_view_buffer_released(host->view, *buffer);
    g_clear_object(buffer);
}
static gboolean publish(WebHost *host, WPEBuffer *buffer, GError **error)
{
    int width = wpe_buffer_get_width(buffer), height = wpe_buffer_get_height(buffer);
    if (width < 1 || height < 1 || width > (int)WEB_MAX_WIDTH || height > (int)WEB_MAX_HEIGHT) {
        g_set_error_literal(error, WPE_VIEW_ERROR, WPE_VIEW_ERROR_RENDER_FAILED, "Invalid web buffer size");
        return FALSE;
    }
    if (WPE_IS_BUFFER_DMA_BUF(buffer)) {
        guint32 format = wpe_buffer_dma_buf_get_format(WPE_BUFFER_DMA_BUF(buffer));
        if (format != DRM_FORMAT_ARGB8888 && format != DRM_FORMAT_XRGB8888) {
            g_set_error_literal(error, WPE_VIEW_ERROR, WPE_VIEW_ERROR_RENDER_FAILED, "Unsupported web buffer format");
            return FALSE;
        }
    }
    GBytes *bytes = wpe_buffer_import_to_pixels(buffer, error);
    if (!bytes) return FALSE;
    gsize size = 0;
    const guint8 *source = g_bytes_get_data(bytes, &size);
    size_t stride = size / (size_t)height;
    if (WPE_IS_BUFFER_SHM(buffer)) stride = wpe_buffer_shm_get_stride(WPE_BUFFER_SHM(buffer));
    size_t row = (size_t)width * 4;
    if (stride < row || stride > size / (size_t)height) {
        g_set_error_literal(error, WPE_VIEW_ERROR, WPE_VIEW_ERROR_RENDER_FAILED, "Invalid web buffer stride");
        return FALSE;
    }
    /* Convert while copying: Slint can consume RGBA directly, avoiding a
     * separate full-frame swizzle in the GUI process. Buffers are opaque. */
    for (int y = 0; y < height; ++y) {
        guint8 *destination = host->pixels + (size_t)y * row;
        const guint8 *line = source + (size_t)y * stride;
        for (size_t x = 0; x < row; x += 4) {
            destination[x] = line[x + 2]; destination[x + 1] = line[x + 1];
            destination[x + 2] = line[x]; destination[x + 3] = 255;
        }
    }
    host->in_flight = g_object_ref(buffer);
    guint8 frame[16];
    guint32 w = GUINT32_TO_BE(width), h = GUINT32_TO_BE(height);
    ++host->sequence;
    guint64 sequence = GUINT64_TO_BE(host->sequence);
    memcpy(frame, &w, 4); memcpy(frame + 4, &h, 4); memcpy(frame + 8, &sequence, 8);
    return web_send(host, 1, frame, sizeof(frame));
}
gboolean web_render(WebHost *host, WPEView *view, WPEBuffer *buffer, GError **error)
{
    host->view = view;
    if (host->in_flight) {
        release_buffer(host, &host->queued);
        host->queued = g_object_ref(buffer);
        return TRUE;
    }
    return publish(host, buffer, error);
}
void web_ack(WebHost *host, uint64_t sequence)
{
    if (!host->in_flight || sequence != host->sequence) return;
    release_buffer(host, &host->in_flight);
    if (host->queued) {
        g_autoptr(WPEBuffer) queued = g_steal_pointer(&host->queued);
        g_autoptr(GError) error = NULL;
        if (!publish(host, queued, &error)) {
            web_error(host, error ? error->message : "Web frame delivery failed");
            wpe_view_buffer_rendered(host->view, queued);
            wpe_view_buffer_released(host->view, queued);
        }
    }
}
void web_discard_buffers(WebHost *host)
{
    release_buffer(host, &host->queued);
    release_buffer(host, &host->in_flight);
}
