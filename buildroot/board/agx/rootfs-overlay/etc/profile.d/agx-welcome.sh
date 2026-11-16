# Keep the HDMI login shell concise; retain the existing SSH welcome.
if [ "$(tty 2>/dev/null)" = /dev/tty1 ]; then
    case "$-" in *i*) printf '\n  ANGI OS  |  type agx --help for commands or agx tui for the dashboard.\n\n' ;; esac
else
# Interactive login splash only; keep SSH commands and file transfers clean.
case "$-" in
    *i*)
        if [ -t 1 ] && [ -r /usr/share/agx/logo.ascii ]; then
            if [ -z "${NO_COLOR+x}" ] && [ "${TERM:-dumb}" != dumb ]; then
                # Reveal the wordmark row by row, then pulse it. "ANG" is white and
                # the final "I" (the last 5 columns of every row) is lime.
                case "$TERM" in
                    *256color*|*direct*) agx_lime='\033[1;38;5;154m' ;;
                    *) agx_lime='\033[1;92m' ;;
                esac
                agx_white='\033[1;97m'
                agx_dim='\033[2;37m'
                agx_logo_row() {
                    agx_ang=${3%?????}
                    printf '%b%s%b%s\033[0m\n' "$1" "$agx_ang" "$2" "${3#"$agx_ang"}"
                }
                printf '\n'
                agx_row=0
                while IFS= read -r agx_line || [ -n "$agx_line" ]; do
                    agx_row=$((agx_row + 1))
                    agx_logo_row "$agx_white" "$agx_lime" "$agx_line"
                    sleep 0.035
                done < /usr/share/agx/logo.ascii
                # Repainting requires enough columns and rows to avoid line wrapping.
                agx_size=$(stty size 2>/dev/null)
                agx_rows=${agx_size% *}
                agx_cols=${agx_size#* }
                if [ "${agx_rows:-0}" -ge 15 ] 2>/dev/null && [ "${agx_cols:-0}" -ge 56 ] 2>/dev/null; then
                    sleep 0.08
                    printf '\033[%sA' "$agx_row"
                    while IFS= read -r agx_line || [ -n "$agx_line" ]; do agx_logo_row "$agx_dim" "$agx_dim" "$agx_line"; done < /usr/share/agx/logo.ascii
                    sleep 0.10
                    printf '\033[%sA' "$agx_row"
                    while IFS= read -r agx_line || [ -n "$agx_line" ]; do agx_logo_row "$agx_white" "$agx_lime" "$agx_line"; done < /usr/share/agx/logo.ascii
                fi
                printf '\n%b  Welcome to ANGI\033[0m  /  %s@%s\n' "$agx_lime" "$(id -un)" "$(hostname)"
                unset agx_line agx_row agx_size agx_rows agx_cols agx_ang agx_lime agx_white agx_dim
                unset -f agx_logo_row 2>/dev/null
            else
                printf '\n'
                cat /usr/share/agx/logo.ascii
                printf '\n  Welcome to ANGI  /  %s@%s\n' "$(id -un)" "$(hostname)"
            fi
            printf '  Open your console: agx tui\n  Profile  /  Hardware  /  Network  /  Reboot\n\n'
        fi
        ;;
esac

fi
