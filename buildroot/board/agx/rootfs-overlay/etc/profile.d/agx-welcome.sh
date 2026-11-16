# Interactive login splash only; keep SSH commands and file transfers clean.
case "$-" in
    *i*)
        if [ -t 1 ] && [ -r /usr/share/agx/logo.ascii ]; then
            if [ -z "${NO_COLOR+x}" ] && [ "${TERM:-dumb}" != dumb ]; then
                printf '\033[1;36m\n'
                # Reveal the logo line by line, including a final line without a newline.
                while IFS= read -r agx_line || [ -n "$agx_line" ]; do
                    printf '%s\n' "$agx_line"
                    sleep 0.04
                done < /usr/share/agx/logo.ascii
                unset agx_line
                printf '\033[0m\n\033[1;35m  Welcome to AGX\033[0m  /  %s@%s\n' "$(id -un)" "$(hostname)"
            else
                printf '\n'
                cat /usr/share/agx/logo.ascii
                printf '\n  Welcome to AGX  /  %s@%s\n' "$(id -un)" "$(hostname)"
            fi
            printf '  Open your console: agx tui\n  Profile  /  Hardware  /  Network  /  Reboot\n\n'
        fi
        ;;
esac
