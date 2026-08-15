# bash completion for ssh-cli (T145)
_ssh_cli() {
    local cur
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    case "${COMP_WORDS[1]}" in
        config)
            COMPREPLY=( $(compgen -W "--check" -- "$cur") )
            return 0
            ;;
        cap)
            COMPREPLY=( $(compgen -W "forward sftp proxy" -- "$cur") )
            return 0
            ;;
    esac
    COMPREPLY=( $(compgen -W "--version --help --json config cap" -- "$cur") )
    return 0
}
complete -F _ssh_cli ssh-cli