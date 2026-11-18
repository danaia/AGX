/* Weston shell adapter for the ANGI window manager's native Chromium tile. */
#include <libweston/desktop.h>
#include <libweston/libweston.h>
#include <libweston/shell-utils.h>
#include <linux/input-event-codes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct tile {
	int state, x, y, width, height, focus, mode;
};

struct shell {
	struct weston_compositor *compositor;
	struct weston_desktop *desktop;
	struct weston_layer background, interface, applications, hidden;
	struct weston_curtain *curtain;
	struct wl_listener destroy;
	struct wl_event_source *timer;
	struct wl_list windows;
	struct window *angi, *browser, *focused;
	struct tile tile;
	const char *runtime;
	unsigned int serial;
	const char *event;
};

struct window {
	struct weston_desktop_surface *surface;
	struct weston_view *view;
	struct wl_list link;
	int width, height, mode;
	bool close_sent;
};

static FILE *open_state(struct shell *shell, const char *name, const char *mode)
{
	char path[4096];
	int size = snprintf(path, sizeof(path), "%s/%s", shell->runtime, name);
	return size > 0 && (size_t)size < sizeof(path) ? fopen(path, mode) : NULL;
}

static void publish(struct shell *shell, const char *event)
{
	if (event) {
		shell->serial++;
		shell->event = event;
	}
	FILE *file = open_state(shell, "angi-compositor-state.new", "w");
	if (!file)
		return;
	fprintf(file, "%d %u %s\n", shell->browser != NULL, shell->serial, shell->event);
	if (fclose(file))
		return;
	char source[4096], target[4096];
	snprintf(source, sizeof(source), "%s/angi-compositor-state.new", shell->runtime);
	snprintf(target, sizeof(target), "%s/angi-compositor-state", shell->runtime);
	rename(source, target);
}

static void focus(struct shell *shell, struct window *window, struct weston_seat *seat)
{
	if (!window || !weston_view_is_mapped(window->view))
		return;
	if (shell->focused != window) {
		if (shell->focused)
			weston_desktop_surface_set_activated(shell->focused->surface, false);
		shell->focused = window;
		weston_desktop_surface_set_activated(window->surface, true);
		if (!seat) {
			wl_list_for_each(seat, &shell->compositor->seat_list, link)
				weston_view_activate_input(window->view, seat, WESTON_ACTIVATE_FLAG_NONE);
			return;
		}
	}
	if (seat)
		weston_view_activate_input(window->view, seat, WESTON_ACTIVATE_FLAG_NONE);
}

static void place(struct window *window, struct weston_output *output,
		  struct weston_layer *layer, int x, int y, int w, int h)
{
	if (window->width != w || window->height != h) {
		window->width = w;
		window->height = h;
		weston_desktop_surface_set_size(window->surface, w, h);
	}
	struct weston_surface *surface = weston_desktop_surface_get_surface(window->surface);
	if (surface->width <= 0)
		return;
	struct weston_geometry geometry = weston_desktop_surface_get_geometry(window->surface);
	struct weston_coord_global position = { .c = { x, y } };
	struct weston_coord_surface offset = weston_coord_surface(-geometry.x, -geometry.y, surface);
	weston_view_set_output(window->view, output);
	weston_view_set_position_with_offset(window->view, position, offset);
	weston_view_set_mask(window->view, geometry.x, geometry.y, w, h);
	weston_view_move_to_layer(window->view, &layer->view_list);
	weston_desktop_surface_propagate_layer(window->surface);
	weston_view_update_transform(window->view);
	if (!weston_surface_is_mapped(surface))
		weston_surface_map(surface);
	weston_view_schedule_repaint(window->view);
}

static struct weston_output *first_output(struct shell *shell)
{
	struct weston_output *output;
	wl_list_for_each(output, &shell->compositor->output_list, link)
		if (!output->mirror_of)
			return output;
	return NULL;
}

static void arrange(struct shell *shell)
{
	struct weston_output *output = first_output(shell);
	if (!output)
		return;
	if (shell->angi)
		place(shell->angi, output, &shell->interface, 0, 0, output->width, output->height);

	struct window *browser = shell->browser;
	struct tile *tile = &shell->tile;
	bool visible = browser && tile->state == 1 && tile->width > 0 && tile->height > 0;
	if (visible) {
		browser->close_sent = false;
		if (browser->mode != tile->mode) {
			browser->mode = tile->mode;
			weston_desktop_surface_set_fullscreen(browser->surface, tile->mode == 2);
			weston_desktop_surface_set_maximized(browser->surface, tile->mode == 1);
		}
		place(browser, output, &shell->applications, tile->x, tile->y, tile->width, tile->height);
	} else if (browser && weston_view_is_mapped(browser->view)) {
		weston_view_move_to_layer(browser->view, &shell->hidden.view_list);
		weston_desktop_surface_propagate_layer(browser->surface);
	}
	focus(shell, visible && tile->focus ? browser : shell->angi, NULL);
}

static int synchronize(void *data)
{
	struct shell *shell = data;
	bool changed = false;
	struct weston_output *output = first_output(shell);
	if (output && !output->ready) {
		struct weston_curtain_params params = {
			.a = 1, .pos = output->pos, .width = output->width, .height = output->height,
		};
		shell->curtain = weston_shell_utils_curtain_create(shell->compositor, &params);
		if (shell->curtain) {
			weston_view_move_to_layer(shell->curtain->view, &shell->background.view_list);
			weston_view_set_output(shell->curtain->view, output);
			weston_output_set_ready(output);
			changed = true;
		}
	}
	if (output && output->ready) {
		struct weston_output *mirror;
		wl_list_for_each(mirror, &shell->compositor->output_list, link) {
			if (mirror->mirror_of == output && !mirror->ready) {
				weston_output_set_ready(mirror);
				changed = true;
			}
		}
	}
	int pid = 0;
	FILE *file = open_state(shell, "angi.pid", "r");
	if (file) {
		if (fscanf(file, "%d", &pid) != 1)
			pid = 0;
		fclose(file);
	}

	struct window *window;
	wl_list_for_each(window, &shell->windows, link) {
		if (!shell->angi && pid > 0 && weston_desktop_surface_get_pid(window->surface) == pid) {
			shell->angi = window;
			weston_desktop_surface_set_fullscreen(window->surface, true);
			changed = true;
		} else if (!shell->browser && window != shell->angi) {
			const char *id = weston_desktop_surface_get_app_id(window->surface);
			if (id && strncmp(id, "chromium", 8) == 0) {
				shell->browser = window;
				changed = true;
				weston_desktop_surface_set_orientation(window->surface,
					WESTON_TOP_LEVEL_TILED_ORIENTATION_LEFT |
					WESTON_TOP_LEVEL_TILED_ORIENTATION_RIGHT |
					WESTON_TOP_LEVEL_TILED_ORIENTATION_TOP |
					WESTON_TOP_LEVEL_TILED_ORIENTATION_BOTTOM);
				publish(shell, NULL);
			}
		}
	}

	struct tile tile;
	file = open_state(shell, "angi-compositor-layout", "r");
	if (file) {
		if (fscanf(file, "%d %d %d %d %d %d %d", &tile.state, &tile.x, &tile.y,
			   &tile.width, &tile.height, &tile.focus, &tile.mode) == 7) {
			if (shell->tile.state && !tile.state && shell->browser && !shell->browser->close_sent) {
				shell->browser->close_sent = true;
				weston_desktop_surface_close(shell->browser->surface);
			}
			if (memcmp(&shell->tile, &tile, sizeof(tile)) != 0) {
				shell->tile = tile;
				changed = true;
			}
		}
		fclose(file);
	}
	if (changed) {
		arrange(shell);
		weston_compositor_damage_all(shell->compositor);
	}
	wl_event_source_timer_update(shell->timer, 30);
	return 0;
}

static void added(struct weston_desktop_surface *surface, void *data)
{
	struct shell *shell = data;
	struct window *window = calloc(1, sizeof(*window));
	if (!window)
		return;
	window->view = weston_desktop_surface_create_view(surface);
	if (!window->view) {
		free(window);
		return;
	}
	window->surface = surface;
	weston_desktop_surface_set_user_data(surface, window);
	weston_surface_set_label_func(window->view->surface, weston_shell_utils_surface_get_label);
	wl_list_insert(shell->windows.prev, &window->link);
}

static void removed(struct weston_desktop_surface *surface, void *data)
{
	struct shell *shell = data;
	struct window *window = weston_desktop_surface_get_user_data(surface);
	if (!window)
		return;
	if (shell->focused == window)
		shell->focused = NULL;
	if (shell->angi == window)
		shell->angi = NULL;
	if (shell->browser == window) {
		shell->browser = NULL;
		publish(shell, NULL);
	}
	wl_list_remove(&window->link);
	weston_desktop_surface_set_user_data(surface, NULL);
	weston_surface_set_label_func(window->view->surface, NULL);
	weston_desktop_surface_unlink_view(window->view);
	weston_view_destroy(window->view);
	free(window);
}

static void committed(struct weston_desktop_surface *surface, struct weston_coord_surface offset, void *data)
{
	struct shell *shell = data;
	(void)surface;
	(void)offset;
	arrange(shell);
}

static void fullscreen(struct weston_desktop_surface *surface, bool full,
		       struct weston_output *output, void *data)
{
	struct shell *shell = data;
	(void)output;
	if (shell->angi && shell->angi->surface == surface) {
		weston_desktop_surface_set_fullscreen(surface, true);
		arrange(shell);
	} else if (shell->browser && shell->browser->surface == surface) {
		publish(shell, full ? "fullscreen" : "restore");
	}
}

static void maximize(struct weston_desktop_surface *surface, bool enabled, void *data)
{
	struct shell *shell = data;
	if (shell->browser && shell->browser->surface == surface)
		publish(shell, enabled ? "maximize" : "restore");
}

static void minimize(struct weston_desktop_surface *surface, void *data)
{
	struct shell *shell = data;
	if (shell->browser && shell->browser->surface == surface)
		publish(shell, "minimize");
}

static const struct weston_desktop_api api = {
	.struct_size = sizeof(api),
	.surface_added = added,
	.surface_removed = removed,
	.committed = committed,
	.fullscreen_requested = fullscreen,
	.maximized_requested = maximize,
	.minimized_requested = minimize,
};

static void clicked(struct weston_pointer *pointer, const struct timespec *time,
		    uint32_t button, void *data)
{
	struct shell *shell = data;
	(void)time;
	(void)button;
	if (!pointer->focus)
		return;
	if (shell->browser && pointer->focus == shell->browser->view) {
		focus(shell, shell->browser, pointer->seat);
		shell->tile.focus = 1;
		publish(shell, "browser");
	} else if (shell->angi && pointer->focus == shell->angi->view) {
		focus(shell, shell->angi, pointer->seat);
		shell->tile.focus = 0;
	}
}

static const struct shortcut {
	uint32_t key;
	const char *event;
} shortcuts[] = {
	{ KEY_SPACE, "menu" }, { KEY_Q, "close" }, { KEY_T, "float" }, { KEY_F, "full" },
	{ KEY_L, "layout" }, { KEY_1, "0" }, { KEY_2, "1" }, { KEY_3, "2" },
	{ KEY_4, "3" }, { KEY_5, "4" },
};

static void shortcut(struct weston_keyboard *keyboard, const struct timespec *time,
		     uint32_t key, void *data)
{
	(void)keyboard;
	(void)time;
	for (size_t i = 0; i < sizeof(shortcuts) / sizeof(shortcuts[0]); i++)
		if (shortcuts[i].key == key)
			publish(data, shortcuts[i].event);
}

static void destroy(struct wl_listener *listener, void *data)
{
	struct shell *shell = wl_container_of(listener, shell, destroy);
	(void)data;
	wl_list_remove(&listener->link);
	wl_event_source_remove(shell->timer);
	while (!wl_list_empty(&shell->windows)) {
		struct window *window = wl_container_of(shell->windows.next, window, link);
		removed(window->surface, shell);
	}
	weston_desktop_destroy(shell->desktop);
	if (shell->curtain)
		weston_shell_utils_curtain_destroy(shell->curtain);
	weston_layer_fini(&shell->hidden);
	weston_layer_fini(&shell->applications);
	weston_layer_fini(&shell->interface);
	weston_layer_fini(&shell->background);
	free(shell);
}

WL_EXPORT int wet_shell_init(struct weston_compositor *compositor, int *argc, char *argv[])
{
	(void)argc;
	(void)argv;
	const char *runtime = getenv("XDG_RUNTIME_DIR");
	if (!runtime || strlen(runtime) >= 4000)
		return -1;
	struct shell *shell = calloc(1, sizeof(*shell));
	if (!shell)
		return -1;
	shell->runtime = runtime;
	shell->event = "-";
	shell->compositor = compositor;
	wl_list_init(&shell->windows);
	weston_layer_init(&shell->background, compositor);
	weston_layer_init(&shell->interface, compositor);
	weston_layer_init(&shell->applications, compositor);
	weston_layer_init(&shell->hidden, compositor);
	weston_layer_set_position(&shell->background, WESTON_LAYER_POSITION_BACKGROUND);
	weston_layer_set_position(&shell->interface, WESTON_LAYER_POSITION_BOTTOM_UI);
	weston_layer_set_position(&shell->applications, WESTON_LAYER_POSITION_NORMAL);
	weston_layer_set_position(&shell->hidden, WESTON_LAYER_POSITION_HIDDEN);
	shell->desktop = weston_desktop_create(compositor, &api, shell);
	struct wl_event_loop *loop = wl_display_get_event_loop(compositor->wl_display);
	shell->timer = wl_event_loop_add_timer(loop, synchronize, shell);
	if (!shell->desktop || !shell->timer)
		return -1;
	weston_compositor_add_destroy_listener_once(compositor, &shell->destroy, destroy);
	for (size_t i = 0; i < sizeof(shortcuts) / sizeof(shortcuts[0]); i++)
		weston_compositor_add_key_binding(compositor, shortcuts[i].key, MODIFIER_SUPER, shortcut, shell);
	weston_compositor_add_button_binding(compositor, BTN_LEFT, 0, clicked, shell);
	wl_event_source_timer_update(shell->timer, 1);
	publish(shell, NULL);
	return 0;
}
