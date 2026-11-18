/* Web form popups and script dialogs use native ANGI chrome, never nested loops. */
#include "host.h"
#include <string.h>
static size_t text_size(const char *text, size_t limit)
{
    if (!text) return 0;
    size_t size = MIN(strlen(text), limit);
    while (size && ((guint8)text[size] & 0xc0) == 0x80) --size;
    return size;
}
static void append_u16(GByteArray *bytes, guint16 value)
{
    value = GUINT16_TO_BE(value); g_byte_array_append(bytes, (guint8 *)&value, 2);
}
static void menu_closed(WebKitOptionMenu *menu, gpointer data)
{
    WebHost *host = data;
    if (host->menu != menu) return;
    g_clear_object(&host->menu);
    web_send(host, 10, NULL, 0);
}
static gboolean show_menu(WebKitWebView *web, WebKitOptionMenu *menu, WebKitRectangle *rectangle, gpointer data)
{
    (void)rectangle;
    WebHost *host = data;
    if (web != host->web) return FALSE;
    if (host->menu) webkit_option_menu_close(host->menu);
    g_clear_pointer(&host->menu_actions, g_ptr_array_unref);
    guint count = webkit_option_menu_get_n_items(menu);
    if (!count || count > 256) { web_error(host, "This selection has too many options to display."); return FALSE; }
    host->menu = g_object_ref(menu);
    g_signal_connect(menu, "close", G_CALLBACK(menu_closed), host);
    g_autoptr(GByteArray) bytes = g_byte_array_new();
    append_u16(bytes, count);
    for (guint i = 0; i < count; ++i) {
        WebKitOptionMenuItem *item = webkit_option_menu_get_item(menu, i);
        const char *label = webkit_option_menu_item_get_label(item);
        size_t size = text_size(label, 200);
        guint8 flags = (webkit_option_menu_item_is_enabled(item) && !webkit_option_menu_item_is_group_label(item) ? 1 : 0)
            | (webkit_option_menu_item_is_selected(item) ? 2 : 0);
        g_byte_array_append(bytes, &flags, 1); append_u16(bytes, size);
        if (size) g_byte_array_append(bytes, (const guint8 *)label, size);
    }
    web_send(host, 8, bytes->data, bytes->len);
    return TRUE;
}
/* Right-click on an image or link: a native list of saves and opens. Each entry is an action letter
 * ('s' save, 'o' open) followed by the address, so a choice can be carried out after the popup closes. */
static gboolean web_address(const char *uri)
{
    return uri && (g_str_has_prefix(uri, "https://") || g_str_has_prefix(uri, "http://")) && strlen(uri) <= 8192;
}
static void add_context(GPtrArray *items, GByteArray *bytes, char action, const char *uri, const char *label)
{
    if (items->len >= 8 || !web_address(uri)) return;
    g_ptr_array_add(items, g_strdup_printf("%c%s", action, uri));
    guint8 flags = 1; size_t size = strlen(label);
    g_byte_array_append(bytes, &flags, 1); append_u16(bytes, size);
    g_byte_array_append(bytes, (const guint8 *)label, size);
}
static gboolean context_menu(WebKitWebView *web, WebKitContextMenu *menu, WebKitHitTestResult *hit, gpointer data)
{
    (void)menu;
    WebHost *host = data;
    g_printerr("agx-web: context-menu image=%s link=%s\n", webkit_hit_test_result_get_image_uri(hit) ?: "-",
        webkit_hit_test_result_get_link_uri(hit) ?: "-");
    if (web != host->web || host->dialog) return TRUE;
    web_menu_response(host, G_MAXUINT16);
    g_autoptr(GPtrArray) items = g_ptr_array_new_with_free_func(g_free);
    g_autoptr(GByteArray) bytes = g_byte_array_new();
    append_u16(bytes, 0);
    if (webkit_hit_test_result_context_is_image(hit)) {
        add_context(items, bytes, 's', webkit_hit_test_result_get_image_uri(hit), "Save image to Downloads");
        add_context(items, bytes, 'o', webkit_hit_test_result_get_image_uri(hit), "Open image in this tab");
    }
    if (webkit_hit_test_result_context_is_link(hit)) {
        add_context(items, bytes, 's', webkit_hit_test_result_get_link_uri(hit), "Save linked file to Downloads");
        add_context(items, bytes, 'o', webkit_hit_test_result_get_link_uri(hit), "Open link in this tab");
    }
    if (!items->len) return TRUE;
    guint16 count = GUINT16_TO_BE(items->len); memcpy(bytes->data, &count, 2);
    host->menu_actions = g_steal_pointer(&items);
    web_send(host, 8, bytes->data, bytes->len);
    return TRUE;
}
static gboolean script_dialog(WebKitWebView *web, WebKitScriptDialog *dialog, gpointer data)
{
    WebHost *host = data;
    if (web != host->web) return TRUE;
    if (host->dialog) web_dialog_response(host, FALSE, "");
    host->dialog = webkit_script_dialog_ref(dialog);
    guint8 type = webkit_script_dialog_get_dialog_type(dialog);
    const char *message = webkit_script_dialog_get_message(dialog);
    const char *value = type == WEBKIT_SCRIPT_DIALOG_PROMPT ? webkit_script_dialog_prompt_get_default_text(dialog) : "";
    size_t size = text_size(message, 8192), value_size = text_size(value, 8192);
    g_autoptr(GByteArray) bytes = g_byte_array_new();
    g_byte_array_append(bytes, &type, 1); append_u16(bytes, size);
    if (size) g_byte_array_append(bytes, (const guint8 *)message, size);
    if (value_size) g_byte_array_append(bytes, (const guint8 *)value, value_size);
    web_send(host, 9, bytes->data, bytes->len);
    return TRUE;
}
static gboolean navigation_policy(WebKitWebView *web, WebKitPolicyDecision *decision, WebKitPolicyDecisionType type, gpointer data)
{
    (void)data;
    if (type != WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION) return FALSE;
    WebKitNavigationAction *action = webkit_navigation_policy_decision_get_navigation_action(WEBKIT_NAVIGATION_POLICY_DECISION(decision));
    const char *uri = webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
    gboolean open = webkit_navigation_action_is_user_gesture(action)
        && (g_str_has_prefix(uri, "https://") || g_str_has_prefix(uri, "http://"));
    g_autofree char *address = open ? g_strdup(uri) : NULL;
    webkit_policy_decision_ignore(decision);
    if (address) webkit_web_view_load_uri(web, address);
    return TRUE;
}
void web_menu_response(WebHost *host, guint16 index)
{
    if (host->menu_actions) {
        g_autoptr(GPtrArray) items = g_steal_pointer(&host->menu_actions);
        if (index < items->len) {
            const char *entry = g_ptr_array_index(items, index);
            if (entry[0] == 's') webkit_web_view_download_uri(host->web, entry + 1);
            else if (entry[0] == 'o') webkit_web_view_load_uri(host->web, entry + 1);
        }
        web_send(host, 10, NULL, 0);
        return;
    }
    if (!host->menu) return;
    g_autoptr(WebKitOptionMenu) menu = g_object_ref(host->menu);
    if (index < webkit_option_menu_get_n_items(menu)) {
        WebKitOptionMenuItem *item = webkit_option_menu_get_item(menu, index);
        if (webkit_option_menu_item_is_enabled(item) && !webkit_option_menu_item_is_group_label(item))
            webkit_option_menu_activate_item(menu, index);
    }
    webkit_option_menu_close(menu);
}
void web_dialog_response(WebHost *host, gboolean accepted, const char *text)
{
    if (!host->dialog) return;
    WebKitScriptDialog *dialog = host->dialog; host->dialog = NULL;
    WebKitScriptDialogType type = webkit_script_dialog_get_dialog_type(dialog);
    if (type == WEBKIT_SCRIPT_DIALOG_BEFORE_UNLOAD_CONFIRM && !accepted) host->closing_tab = FALSE;
    if (type == WEBKIT_SCRIPT_DIALOG_CONFIRM || type == WEBKIT_SCRIPT_DIALOG_BEFORE_UNLOAD_CONFIRM)
        webkit_script_dialog_confirm_set_confirmed(dialog, accepted);
    if (type == WEBKIT_SCRIPT_DIALOG_PROMPT && accepted)
        webkit_script_dialog_prompt_set_text(dialog, text);
    webkit_script_dialog_close(dialog); webkit_script_dialog_unref(dialog);
    web_send(host, 10, NULL, 0);
}
static void close_window(WebKitWebView *web, gpointer data)
{
    web_tab_closed(data, web);
}
void web_dialogs_connect(WebHost *host, WebKitWebView *web)
{
    g_signal_connect(web, "close", G_CALLBACK(close_window), host);
    g_signal_connect(web, "show-option-menu", G_CALLBACK(show_menu), host);
    g_signal_connect(web, "context-menu", G_CALLBACK(context_menu), host);
    g_signal_connect(web, "script-dialog", G_CALLBACK(script_dialog), host);
    g_signal_connect(web, "decide-policy", G_CALLBACK(navigation_policy), host);
}
