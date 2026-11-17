# Defaults apply only to ANGI's native terminal; personal shell files run afterward.
if [ "$TERM_PROGRAM" = ANGI ] && [ -n "$BASH_VERSION" ]; then
    case $- in
        *i*)
            HISTSIZE=5000
            HISTFILESIZE=10000
            HISTCONTROL=ignoreboth
            shopt -s histappend checkwinsize
            PS1='\[\e[38;5;150m\]\u@\h\[\e[0m\]:\[\e[38;5;110m\]\w\[\e[0m\]\$ '
            export PAGER=less
            export LESS='-FRX'
            if [ -x /usr/bin/vim ]; then export EDITOR=/usr/bin/vim; fi
            ;;
    esac
fi
