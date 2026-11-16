# Keep the HDMI login shell concise; retain the existing SSH welcome.
if [ "$(tty 2>/dev/null)" = /dev/tty1 ]; then
    case "$-" in *i*) printf '\n  AGX OS  |  type agx --help for commands or agx tui for the dashboard.\n\n' ;; esac
else
# Interactive login splash only; keep SSH commands and file transfers clean.
case "$-" in
    *i*)
        if [ -t 1 ] && [ -r /usr/share/agx/logo.ascii ]; then
            if [ -z "${NO_COLOR+x}" ] && [ "${TERM:-dumb}" != dumb ]; then
                # Reveal a cyan-to-violet sweep, then let the complete mark pulse.
                printf '\n'
                agx_row=0
                while IFS= read -r agx_line || [ -n "$agx_line" ]; do
                    agx_row=$((agx_row + 1))
                    case "$agx_row" in
                        1|2|3|4) printf '\033[1;36m' ;;
                        5|6|7)   printf '\033[1;34m' ;;
                        *)       printf '\033[1;35m' ;;
                    esac
                    printf '%s\033[0m\n' "$agx_line"
                    sleep 0.035
                done < /usr/share/agx/logo.ascii
                # Repainting requires enough columns and rows to avoid line wrapping.
                agx_size=$(stty size 2>/dev/null)
                agx_rows=${agx_size% *}
                agx_cols=${agx_size#* }
                if [ "${agx_rows:-0}" -ge 15 ] 2>/dev/null && [ "${agx_cols:-0}" -ge 56 ] 2>/dev/null; then
                    sleep 0.08
                    printf '\033[%sA\033[1;35m' "$agx_row"
                    while IFS= read -r agx_line || [ -n "$agx_line" ]; do printf '%s\n' "$agx_line"; done < /usr/share/agx/logo.ascii
                    sleep 0.10
                    printf '\033[%sA\033[1;36m' "$agx_row"
                    while IFS= read -r agx_line || [ -n "$agx_line" ]; do printf '%s\n' "$agx_line"; done < /usr/share/agx/logo.ascii
                    printf '\033[0m'
                fi
                unset agx_line agx_row agx_size agx_rows agx_cols
                printf '\n\033[1;35m  Welcome to AGX\033[0m  /  %s@%s\n' "$(id -un)" "$(hostname)"
            else
                printf '\n'
                cat /usr/share/agx/logo.ascii
                printf '\n  Welcome to AGX  /  %s@%s\n' "$(id -un)" "$(hostname)"
            fi
            printf '  Open your console: agx tui\n  Profile  /  Hardware  /  Network  /  Reboot\n\n'
        fi
        ;;
esac

fi
